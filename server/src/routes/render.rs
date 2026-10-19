//! 识别触发、进度查询与 Word 下载。

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::Json;
use serde::Serialize;
use serde_json::json;

use crate::blocks::Block;
use crate::device::DeviceId;
use crate::docx::{self, PageDoc};
use crate::error::{AppError, AppResult};
use crate::routes::groups;
use crate::state::AppState;
use crate::storage;

const DOCX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PageStatus {
    pub id: String,
    pub ord: i64,
    pub status: String,
    pub w: i64,
    pub h: i64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusResp {
    pub state: String,
    pub done: i64,
    pub total: i64,
    pub docx_rev: i64,
    pub error: Option<String>,
    pub pages: Vec<PageStatus>,
}

/// `POST /shiye/api/v1/groups/{id}/render`
pub async fn start(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(group_id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    groups::fetch_group(&st, &dev.0, &group_id).await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pages WHERE group_id = ?")
        .bind(&group_id)
        .fetch_one(&st.pool)
        .await?;
    if total == 0 {
        return Err(AppError::BadRequest("组内没有页面".to_string()));
    }
    crate::render::start(st.clone(), &group_id).await?;
    Ok(Json(json!({"state": "queued"})))
}

/// `GET /shiye/api/v1/groups/{id}/status`
pub async fn status(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(group_id): Path<String>,
) -> AppResult<Json<StatusResp>> {
    let group = groups::fetch_group(&st, &dev.0, &group_id).await?;
    let pages = sqlx::query_as::<_, PageStatus>(
        "SELECT id, ord, status, w, h, error FROM pages WHERE group_id = ? ORDER BY ord ASC",
    )
    .bind(&group_id)
    .fetch_all(&st.pool)
    .await?;

    let error = sqlx::query_scalar::<_, Option<String>>("SELECT error FROM groups WHERE id = ?")
        .bind(&group_id)
        .fetch_one(&st.pool)
        .await
        .unwrap_or(None);

    Ok(Json(StatusResp {
        state: group.state,
        done: group.done_count,
        total: group.page_count,
        docx_rev: group.docx_rev,
        error,
        pages,
    }))
}

/// `GET /shiye/api/v1/groups/{id}/docx`（缺失时按需生成并缓存）
pub async fn download_docx(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(group_id): Path<String>,
) -> AppResult<Response> {
    let group = groups::fetch_group(&st, &dev.0, &group_id).await?;
    if group.page_count == 0 {
        return Err(AppError::BadRequest("组内没有页面".to_string()));
    }

    if group.docx_rev > 0 {
        let path = storage::docx_path(&st.cfg.data_dir, &group_id, group.docx_rev);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok(attachment(&group.title, bytes));
        }
    }

    let pages = load_pages(&st, &group_id).await?;
    if pages.iter().all(|p| p.blocks.is_empty()) {
        return Err(AppError::BadRequest(
            "尚未识别出内容，请先生成".to_string(),
        ));
    }

    let data_dir = st.cfg.data_dir.clone();
    let title = group.title.clone();
    let gid = group_id.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        let load_fig = |page_id: &str, name: &str| -> Option<Vec<u8>> {
            std::fs::read(storage::page_file(&data_dir, gid.as_str(), page_id, name)).ok()
        };
        docx::build(&title, &pages, &load_fig)
    })
    .await
    .map_err(AppError::internal)?
    .map_err(AppError::internal)?;

    let rev = group.docx_rev + 1;
    storage::write_atomic(
        &storage::docx_path(&st.cfg.data_dir, &group_id, rev),
        &bytes,
    )?;
    sqlx::query("UPDATE groups SET docx_rev = ?, updated_at = ? WHERE id = ?")
        .bind(rev)
        .bind(crate::db::now())
        .bind(&group_id)
        .execute(&st.pool)
        .await?;

    Ok(attachment(&group.title, bytes))
}

async fn load_pages(st: &AppState, group_id: &str) -> AppResult<Vec<PageDoc>> {
    let pages: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT id, w, h FROM pages WHERE group_id = ? AND status = 'done' ORDER BY ord ASC",
    )
    .bind(group_id)
    .fetch_all(&st.pool)
    .await?;

    let mut out = Vec::with_capacity(pages.len());
    for (page_id, w, h) in pages {
        let blocks = sqlx::query_as::<_, Block>(
            "SELECT ord, label, text, md, x1, y1, x2, y2, poly
             FROM blocks WHERE page_id = ? ORDER BY ord ASC",
        )
        .bind(&page_id)
        .fetch_all(&st.pool)
        .await?;
        out.push(PageDoc {
            page_id,
            w: w as f64,
            h: h as f64,
            blocks,
        });
    }
    Ok(out)
}

fn attachment(title: &str, bytes: Vec<u8>) -> Response {
    let name = sanitize_filename(title);
    let disposition = format!(
        "attachment; filename=\"{name}.docx\"; filename*=UTF-8''{}",
        percent_encode(&format!("{name}.docx"))
    );
    let mut resp = Response::new(Body::from(bytes));
    *resp.status_mut() = StatusCode::OK;
    let headers = resp.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(DOCX_MIME));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    resp
}

fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "拾页文档".to_string()
    } else {
        trimmed.chars().take(60).collect()
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_is_sanitized() {
        assert_eq!(sanitize_filename("a/b:c"), "a_b_c");
        assert_eq!(sanitize_filename("  ...  "), "拾页文档");
    }

    #[test]
    fn percent_encoding_keeps_unreserved() {
        assert_eq!(percent_encode("a-1.docx"), "a-1.docx");
        assert_eq!(percent_encode("拾页"), "%E6%8B%BE%E9%A1%B5");
    }
}
