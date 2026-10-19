//! 页面上传与删除。
//!
//! 客户端在端侧完成透视矫正，上传的即最终 JPEG。服务端只做派生图（disp/thumb）
//! 与落盘，不做图像处理。页序即上传顺序，之后由 `PATCH /groups/{id}` 重排。

use std::path::PathBuf;

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use crate::db;
use crate::device::DeviceId;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::storage;

#[derive(Debug, Serialize)]
pub struct UploadedPage {
    pub page_id: String,
    pub ord: i64,
    pub w: u32,
    pub h: u32,
}

/// `POST /shiye/api/v1/groups/{id}/pages`（multipart：`file` 必填，`raw` 可选）
pub async fn upload(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(group_id): Path<String>,
    mut form: Multipart,
) -> AppResult<(StatusCode, Json<UploadedPage>)> {
    crate::routes::groups::fetch_group(&st, &dev.0, &group_id).await?;
    storage::safe_id(&group_id)?;

    let mut file: Option<Vec<u8>> = None;
    let mut raw: Option<Vec<u8>> = None;
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart 解析失败: {e}")))?
    {
        match field.name().unwrap_or_default() {
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("读取 file 失败: {e}")))?;
                file = Some(bytes.to_vec());
            }
            "raw" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("读取 raw 失败: {e}")))?;
                raw = Some(bytes.to_vec());
            }
            _ => {}
        }
    }

    let jpeg = file.ok_or_else(|| AppError::BadRequest("缺少 file 字段".to_string()))?;
    if jpeg.len() < 128 {
        return Err(AppError::BadRequest("图片内容为空".to_string()));
    }

    // 解码 + 派生图是 CPU 密集操作，放到阻塞线程池。
    let (disp_edge, thumb_edge, max_pixels) = (
        st.cfg.display_max_edge,
        st.cfg.thumb_max_edge,
        st.cfg.max_image_pixels,
    );
    let probe = jpeg.clone();
    let (disp, thumb, w, h) =
        tokio::task::spawn_blocking(move || {
            crate::imaging::derive(&probe, disp_edge, thumb_edge, max_pixels)
        })
        .await
        .map_err(AppError::internal)??;

    let page_id = uuid::Uuid::new_v4().to_string();
    let dir = storage::page_dir(&st.cfg.data_dir, &group_id, &page_id);
    let ts = db::now();

    let mut tx = st.pool.begin().await?;
    // 归属校验放在同一个事务里，避免 TOCTOU。
    let ord: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(ord) + 1, 0) FROM pages WHERE group_id = ?",
    )
    .bind(&group_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO pages (id, group_id, ord, status, w, h, bytes, created_at)
         VALUES (?, ?, ?, 'pending', ?, ?, ?, ?)",
    )
    .bind(&page_id)
    .bind(&group_id)
    .bind(ord)
    .bind(w as i64)
    .bind(h as i64)
    .bind(jpeg.len() as i64)
    .bind(ts)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE groups SET page_count = (SELECT COUNT(*) FROM pages WHERE group_id = ?),
                          state = 'draft', updated_at = ?
         WHERE id = ?",
    )
    .bind(&group_id)
    .bind(ts)
    .bind(&group_id)
    .execute(&mut *tx)
    .await?;

    // 页面变化后旧 docx 失效。
    sqlx::query("UPDATE groups SET docx_rev = 0 WHERE id = ?")
        .bind(&group_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    write_page_files(dir, jpeg, raw, disp, thumb).await?;

    Ok((
        StatusCode::CREATED,
        Json(UploadedPage {
            page_id,
            ord,
            w,
            h,
        }),
    ))
}

async fn write_page_files(
    dir: PathBuf,
    edit: Vec<u8>,
    raw: Option<Vec<u8>>,
    disp: Vec<u8>,
    thumb: Vec<u8>,
) -> AppResult<()> {
    tokio::task::spawn_blocking(move || -> AppResult<()> {
        storage::write_atomic(&dir.join("edit.jpg"), &edit)?;
        if let Some(raw) = raw {
            storage::write_atomic(&dir.join("raw.jpg"), &raw)?;
        }
        storage::write_atomic(&dir.join("disp.jpg"), &disp)?;
        storage::write_atomic(&dir.join("thumb.jpg"), &thumb)?;
        Ok(())
    })
    .await
    .map_err(AppError::internal)?
}

/// `DELETE /shiye/api/v1/pages/{id}`
pub async fn remove(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(page_id): Path<String>,
) -> AppResult<StatusCode> {
    storage::safe_id(&page_id)?;
    let group_id: Option<String> = sqlx::query_scalar(
        "SELECT p.group_id FROM pages p JOIN groups g ON g.id = p.group_id
         WHERE p.id = ? AND g.device_id = ?",
    )
    .bind(&page_id)
    .bind(&dev.0)
    .fetch_optional(&st.pool)
    .await?;
    let group_id = group_id.ok_or(AppError::NotFound)?;

    let mut tx = st.pool.begin().await?;
    sqlx::query("DELETE FROM pages WHERE id = ? AND group_id = ?")
        .bind(&page_id)
        .bind(&group_id)
        .execute(&mut *tx)
        .await?;
    renumber(&mut tx, &group_id).await?;
    sqlx::query(
        "UPDATE groups SET page_count = (SELECT COUNT(*) FROM pages WHERE group_id = ?),
                          done_count = (SELECT COUNT(*) FROM pages WHERE group_id = ? AND status = 'done'),
                          docx_rev = 0, state = 'draft', updated_at = ?
         WHERE id = ?",
    )
    .bind(&group_id)
    .bind(&group_id)
    .bind(db::now())
    .bind(&group_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let dir = storage::page_dir(&st.cfg.data_dir, &group_id, &page_id);
    let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&dir)).await;

    Ok(StatusCode::NO_CONTENT)
}

/// 删页后把 `ord` 压成 0..n-1，保证页序连续。
pub async fn renumber(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    group_id: &str,
) -> AppResult<()> {
    sqlx::query(
        "WITH ordered AS (
             SELECT id, ROW_NUMBER() OVER (ORDER BY ord ASC, created_at ASC) - 1 AS new_ord
             FROM pages WHERE group_id = ?
         )
         UPDATE pages SET ord = (SELECT new_ord FROM ordered WHERE ordered.id = pages.id)
         WHERE group_id = ?",
    )
    .bind(group_id)
    .bind(group_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// `PUT /shiye/api/v1/pages/{id}`
///
/// 重拍 / 重新编辑后替换该页图片：派生图重算、旧识别结果作废、组回到 `draft`。
pub async fn replace(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(page_id): Path<String>,
    mut form: Multipart,
) -> AppResult<Json<UploadedPage>> {
    storage::safe_id(&page_id)?;
    let found: Option<(String, i64)> = sqlx::query_as(
        "SELECT p.group_id, p.ord FROM pages p JOIN groups g ON g.id = p.group_id
         WHERE p.id = ? AND g.device_id = ?",
    )
    .bind(&page_id)
    .bind(&dev.0)
    .fetch_optional(&st.pool)
    .await?;
    let (group_id, ord) = found.ok_or(AppError::NotFound)?;

    let mut file: Option<Vec<u8>> = None;
    let mut raw: Option<Vec<u8>> = None;
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart 解析失败: {e}")))?
    {
        match field.name().unwrap_or_default() {
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("读取 file 失败: {e}")))?;
                file = Some(bytes.to_vec());
            }
            "raw" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("读取 raw 失败: {e}")))?;
                raw = Some(bytes.to_vec());
            }
            _ => {}
        }
    }
    let jpeg = file.ok_or_else(|| AppError::BadRequest("缺少 file 字段".to_string()))?;

    let (disp_edge, thumb_edge, max_pixels) = (
        st.cfg.display_max_edge,
        st.cfg.thumb_max_edge,
        st.cfg.max_image_pixels,
    );
    let probe = jpeg.clone();
    let (disp, thumb, w, h) =
        tokio::task::spawn_blocking(move || {
            crate::imaging::derive(&probe, disp_edge, thumb_edge, max_pixels)
        })
        .await
        .map_err(AppError::internal)??;

    let dir = storage::page_dir(&st.cfg.data_dir, &group_id, &page_id);
    write_page_files(dir, jpeg.clone(), raw, disp, thumb).await?;

    let mut tx = st.pool.begin().await?;
    sqlx::query(
        "UPDATE pages SET status = 'pending', w = ?, h = ?, bytes = ?, error = NULL WHERE id = ?",
    )
    .bind(w as i64)
    .bind(h as i64)
    .bind(jpeg.len() as i64)
    .bind(&page_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM blocks WHERE page_id = ?")
        .bind(&page_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE groups SET state = 'draft', docx_rev = 0,
                           done_count = (SELECT COUNT(*) FROM pages WHERE group_id = ? AND status = 'done'),
                           updated_at = ?
         WHERE id = ?",
    )
    .bind(&group_id)
    .bind(db::now())
    .bind(&group_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(UploadedPage {
        page_id,
        ord,
        w,
        h,
    }))
}
