//! 识别流水线：每个待识别页面一个 `tokio` 任务，`Semaphore` 限制并发。
//!
//! 组状态机：`recognizing` → `done` / `failed`；单页状态：`pending` → `recognizing` →
//! `done` / `failed`。已完成（`done`）的页在重试时会被跳过，天然支持断点续传。

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::baidu::BaiduClient;
use crate::blocks;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::storage;

#[derive(Debug, sqlx::FromRow)]
struct PageRow {
    id: String,
    ord: i64,
}

/// 启动一个组的识别任务；已在跑时直接返回（幂等）。
pub async fn start(state: AppState, group_id: &str) -> AppResult<()> {
    {
        let mut running = state.rendering.lock().expect("rendering 锁中毒");
        if !running.insert(group_id.to_string()) {
            return Ok(());
        }
    }

    if let Err(e) = prepare(&state, group_id).await {
        state.rendering.lock().expect("rendering 锁中毒").remove(group_id);
        return Err(e);
    }

    let gid = group_id.to_string();
    tokio::spawn(async move {
        run(state.clone(), &gid).await;
        state.rendering.lock().expect("rendering 锁中毒").remove(&gid);
    });
    Ok(())
}

/// 标记组状态、重置失败页，并让旧 docx 失效。
async fn prepare(state: &AppState, group_id: &str) -> AppResult<()> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pages WHERE group_id = ?")
        .bind(group_id)
        .fetch_one(&state.pool)
        .await?;
    if total == 0 {
        return Err(AppError::BadRequest("组内没有页面".to_string()));
    }

    let ts = db::now();
    sqlx::query(
        "UPDATE groups SET state = 'recognizing', error = NULL, docx_rev = 0,
                            done_count = (SELECT COUNT(*) FROM pages WHERE group_id = ? AND status = 'done'),
                            updated_at = ?
         WHERE id = ?",
    )
    .bind(group_id)
    .bind(ts)
    .bind(group_id)
    .execute(&state.pool)
    .await?;

    sqlx::query("UPDATE pages SET status = 'pending', error = NULL WHERE group_id = ? AND status <> 'done'")
        .bind(group_id)
        .execute(&state.pool)
        .await?;

    // 旧 Word 作废，磁盘上不再保留中间版本。
    let docx_dir = state.cfg.data_dir.join("g").join(group_id).join("docx");
    let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(docx_dir)).await;
    Ok(())
}

async fn run(state: AppState, group_id: &str) {
    let pages: Vec<PageRow> = match sqlx::query_as(
        "SELECT id, ord FROM pages WHERE group_id = ? AND status <> 'done' ORDER BY ord ASC",
    )
    .bind(group_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, group = group_id, "读取待识别页失败");
            mark_group(&state, group_id, 0).await;
            return;
        }
    };

    let sem = Arc::new(Semaphore::new(state.cfg.ocr_concurrency.max(1)));
    let mut set: JoinSet<bool> = JoinSet::new();

    for page in pages {
        let permit = match Arc::clone(&sem).acquire_owned().await {
            Ok(p) => p,
            Err(_) => break,
        };
        let st = state.clone();
        let gid = group_id.to_string();
        set.spawn(async move {
            let _permit = permit;
            let ok = process_page(&st, &gid, &page).await.is_ok();
            let _ = refresh_done_count(&st, &gid).await;
            ok
        });
    }

    let mut failed = 0usize;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(true) => {}
            Ok(false) => failed += 1,
            Err(e) => {
                tracing::error!(error = %e, "识别任务异常退出");
                failed += 1;
            }
        }
    }
    mark_group(&state, group_id, failed).await;
}

async fn mark_group(state: &AppState, group_id: &str, failed: usize) {
    let state_str = if failed == 0 { "done" } else { "failed" };
    let error = if failed == 0 {
        None
    } else {
        Some(format!("{failed} 页识别失败，可重试"))
    };
    if let Err(e) = sqlx::query(
        "UPDATE groups SET state = ?, error = ?, updated_at = ? WHERE id = ?",
    )
    .bind(state_str)
    .bind(error.as_deref())
    .bind(db::now())
    .bind(group_id)
    .execute(&state.pool)
    .await
    {
        tracing::error!(error = %e, group = group_id, "更新组状态失败");
    }
    let _ = refresh_done_count(state, group_id).await;
}

async fn refresh_done_count(state: &AppState, group_id: &str) -> AppResult<()> {
    sqlx::query(
        "UPDATE groups SET done_count = (SELECT COUNT(*) FROM pages WHERE group_id = ? AND status = 'done')
         WHERE id = ?",
    )
    .bind(group_id)
    .bind(group_id)
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// 单页全流程：百度解析 → 插图落盘 → 块适配 → 入库。
async fn process_page(state: &AppState, group_id: &str, page: &PageRow) -> AppResult<()> {
    set_page_status(state, &page.id, "recognizing", None).await?;

    let dir = storage::page_dir(&state.cfg.data_dir, group_id, &page.id);
    let image = match tokio::fs::read(dir.join("edit.jpg")).await {
        Ok(b) => b,
        Err(e) => {
            let msg = format!("读取页面图片失败: {e}");
            set_page_status(state, &page.id, "failed", Some(&msg)).await?;
            return Err(AppError::internal(msg));
        }
    };

    let started = Instant::now();
    let client = BaiduClient::new(
        &state.http,
        &state.cfg.baidu.api_key,
        &state.cfg.baidu.secret_key,
        &state.pool,
        state.cfg.ocr_poll_interval_secs,
        state.cfg.ocr_timeout_secs,
    );

    let file_name = format!("page{:03}.jpg", page.ord + 1);
    let mut raw_json: Option<Value> = None;
    let mut last_err = String::new();
    for (attempt, delay) in [(0u32, 0u64), (1, 2), (2, 6)] {
        if delay > 0 {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
        match client.parse_page(&image, &file_name).await {
            Ok(v) => {
                raw_json = Some(v);
                break;
            }
            Err(e) => {
                last_err = e.to_string();
                tracing::warn!(attempt, page = %page.id, error = %last_err, "百度解析失败，准备重试");
            }
        }
    }

    let Some(raw_json) = raw_json else {
        set_page_status(state, &page.id, "failed", Some(&last_err)).await?;
        return Err(AppError::internal(last_err));
    };

    let page_json = match extract_page(&raw_json) {
        Ok(p) => p,
        Err(e) => {
            let msg = e.to_string();
            set_page_status(state, &page.id, "failed", Some(&msg)).await?;
            return Err(AppError::internal(msg));
        }
    };

    let fig_names = download_figures(state, &page_json, &dir).await?;
    let adapted = match blocks::adapt_page(&page_json, &fig_names) {
        Ok(a) => a,
        Err(e) => {
            let msg = format!("适配百度结果失败: {e}");
            set_page_status(state, &page.id, "failed", Some(&msg)).await?;
            return Err(AppError::internal(msg));
        }
    };

    let mut blocks_out = adapted.blocks;
    for block in &mut blocks_out {
        if block.md.contains("<img") {
            block.md = localize_html_imgs(state, &block.md, &dir).await;
        }
    }

    let ocr_ms = started.elapsed().as_millis() as i64;
    let mut tx = state.pool.begin().await?;
    sqlx::query("DELETE FROM blocks WHERE page_id = ?")
        .bind(&page.id)
        .execute(&mut *tx)
        .await?;
    for b in &blocks_out {
        sqlx::query(
            "INSERT INTO blocks (page_id, ord, label, text, md, x1, y1, x2, y2, poly)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&page.id)
        .bind(b.ord)
        .bind(&b.label)
        .bind(&b.text)
        .bind(&b.md)
        .bind(b.x1)
        .bind(b.y1)
        .bind(b.x2)
        .bind(b.y2)
        .bind(b.poly.as_deref())
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE pages SET status = 'done', w = ?, h = ?, ocr_ms = ?, error = NULL WHERE id = ?",
    )
    .bind(adapted.width as i64)
    .bind(adapted.height as i64)
    .bind(ocr_ms)
    .bind(&page.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(())
}

async fn set_page_status(
    state: &AppState,
    page_id: &str,
    status: &str,
    error: Option<&str>,
) -> AppResult<()> {
    sqlx::query("UPDATE pages SET status = ?, error = ? WHERE id = ?")
        .bind(status)
        .bind(error)
        .bind(page_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// 百度结果外层可能包一层 `result` / `data`，也可能是单页对象本身。
fn extract_page(json: &Value) -> AppResult<Value> {
    let candidates = [
        "/result/pages/0",
        "/pages/0",
        "/result/0",
        "/data/pages/0",
        "/data/result/pages/0",
    ];
    for ptr in candidates {
        if let Some(page) = json.pointer(ptr) {
            if page.is_object() {
                return Ok(page.clone());
            }
        }
    }
    if json.get("layouts").is_some() {
        return Ok(json.clone());
    }
    Err(AppError::internal(format!(
        "无法从百度结果中定位页面: {}",
        json.to_string().chars().take(200).collect::<String>()
    )))
}

/// 页内插图（`images[].data_url`）落盘，返回与 `images` 同序的文件名。
async fn download_figures(state: &AppState, page: &Value, dir: &Path) -> AppResult<Vec<String>> {
    let mut names = Vec::new();
    let Some(images) = page.get("images").and_then(Value::as_array) else {
        return Ok(names);
    };
    for image in images {
        let Some(data_url) = image.get("data_url").and_then(Value::as_str) else {
            names.push(String::new());
            continue;
        };
        match fetch_image(state, data_url).await {
            Ok(bytes) => {
                let name = fig_name(&bytes);
                storage::write_atomic(&dir.join(&name), &bytes)?;
                names.push(name);
            }
            Err(e) => {
                tracing::warn!(error = %e, "下载插图失败");
                names.push(String::new());
            }
        }
    }
    Ok(names)
}

/// `data:` URL 或远程 http 图片。
async fn fetch_image(state: &AppState, src: &str) -> AppResult<Vec<u8>> {
    if let Some((meta, payload)) = src.split_once(',') {
        if meta.starts_with("data:") {
            let cleaned: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
            return base64::engine::general_purpose::STANDARD
                .decode(cleaned.as_bytes())
                .map_err(|e| AppError::internal(format!("解析 data URL 失败: {e}")));
        }
    }
    let resp = state
        .http
        .get(src)
        .send()
        .await
        .map_err(|e| AppError::internal(format!("下载图片失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::internal(format!(
            "下载图片失败: HTTP {}",
            resp.status()
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::internal(format!("读取图片失败: {e}")))?;
    Ok(bytes.to_vec())
}

/// 文件名只含字母数字，满足 `blocks::sanitize_fig_name`。
fn fig_name(bytes: &[u8]) -> String {
    let ext = if bytes.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else {
        "png"
    };
    format!("fig_{}.{ext}", uuid::Uuid::new_v4().simple())
}

/// 把 markdown 里指向远程地址的 `<img>` 下载到本地并改写成 fig 文件名。
async fn localize_html_imgs(state: &AppState, md: &str, dir: &Path) -> String {
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(pos) = rest.find("<img") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        let tag = &tail[..=end];
        rest = &tail[end + 1..];

        let Some(src) = attr_value(tag, "src") else {
            continue;
        };
        if !(src.starts_with("http://") || src.starts_with("https://")) {
            if let Some(safe) = blocks::sanitize_fig_name(&src) {
                out.push_str(&format!("<img src=\"{safe}\">"));
            }
            continue;
        }
        match fetch_image(state, &src).await {
            Ok(bytes) => {
                let name = fig_name(&bytes);
                if let Err(e) = storage::write_atomic(&dir.join(&name), &bytes) {
                    tracing::warn!(error = %e, "写入表格图片失败");
                    continue;
                }
                out.push_str(&format!("<img src=\"{name}\">"));
            }
            Err(e) => tracing::warn!(error = %e, url = %src, "下载表格内图片失败"),
        }
    }
    out.push_str(rest);
    out
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pos = lower.find(&format!("{attr}="))?;
    let after = &tag[pos + attr.len() + 1..];
    let quote = after.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = after[1..].find(quote)? + 1;
        Some(after[1..end].to_string())
    } else {
        let end = after
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(after.len());
        Some(after[..end].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_nested_pages() {
        let json = serde_json::json!({"result": {"pages": [{"meta": {"page_width": 10}}]}});
        let page = extract_page(&json).unwrap();
        assert_eq!(page.pointer("/meta/page_width").unwrap(), 10);
    }

    #[test]
    fn extracts_single_page_object() {
        let json = serde_json::json!({"layouts": [], "meta": {}});
        assert!(extract_page(&json).is_ok());
    }

    #[test]
    fn rejects_unusable_shape() {
        let json = serde_json::json!({"foo": 1});
        assert!(extract_page(&json).is_err());
    }

    #[test]
    fn fig_names_are_safe() {
        let name = fig_name(&[0xFF, 0xD8, 0x00]);
        assert!(name.starts_with("fig_"));
        assert!(name.ends_with(".jpg"));
        assert!(blocks::sanitize_fig_name(&name).is_some());
        assert!(blocks::sanitize_fig_name(&fig_name(&[0x89])).is_some());
    }

    #[test]
    fn reads_quoted_attr() {
        assert_eq!(
            attr_value("<img src='a.png' width=3>", "src").unwrap(),
            "a.png"
        );
        assert_eq!(
            attr_value("<img src=\"b.png\">", "src").unwrap(),
            "b.png"
        );
    }
}
