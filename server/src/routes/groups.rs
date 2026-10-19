//! 拍照组的增删改查。所有查询都按 `device_id` 限定归属。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::db;
use crate::device::DeviceId;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const DEFAULT_TITLE: &str = "未命名文档";
const MAX_TITLE_CHARS: usize = 60;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Group {
    pub id: String,
    pub title: String,
    pub state: String,
    pub page_count: i64,
    pub done_count: i64,
    pub docx_rev: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PageBrief {
    pub id: String,
    pub ord: i64,
    pub status: String,
    pub w: i64,
    pub h: i64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GroupDetail {
    #[serde(flatten)]
    pub group: Group,
    pub pages: Vec<PageBrief>,
}

#[derive(Debug, Deserialize)]
pub struct CreateGroup {
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchGroup {
    #[serde(default)]
    pub title: Option<String>,
    /// 新的页序（page id 数组），决定 Word 输出顺序。
    #[serde(default)]
    pub page_order: Option<Vec<String>>,
}

fn normalize_title(raw: Option<String>) -> AppResult<String> {
    let title = raw.unwrap_or_default().trim().to_string();
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::BadRequest(format!(
            "标题过长（上限 {MAX_TITLE_CHARS} 字）"
        )));
    }
    Ok(if title.is_empty() {
        DEFAULT_TITLE.to_string()
    } else {
        title
    })
}

/// `POST /shiye/api/v1/groups`
pub async fn create(
    State(st): State<AppState>,
    dev: DeviceId,
    Json(body): Json<CreateGroup>,
) -> AppResult<(StatusCode, Json<Group>)> {
    let title = normalize_title(body.title)?;
    let id = uuid::Uuid::new_v4().to_string();
    let ts = db::now();

    sqlx::query(
        "INSERT INTO groups (id, device_id, title, state, page_count, done_count, docx_rev, created_at, updated_at)
         VALUES (?, ?, ?, 'draft', 0, 0, 0, ?, ?)",
    )
    .bind(&id)
    .bind(&dev.0)
    .bind(&title)
    .bind(ts)
    .bind(ts)
    .execute(&st.pool)
    .await?;

    let group = fetch_group(&st, &dev.0, &id).await?;
    Ok((StatusCode::CREATED, Json(group)))
}

/// `GET /shiye/api/v1/groups`
pub async fn list(State(st): State<AppState>, dev: DeviceId) -> AppResult<Json<Vec<Group>>> {
    let rows = sqlx::query_as::<_, Group>(
        "SELECT id, title, state, page_count, done_count, docx_rev, created_at, updated_at
         FROM groups WHERE device_id = ? ORDER BY created_at DESC LIMIT 500",
    )
    .bind(&dev.0)
    .fetch_all(&st.pool)
    .await?;
    Ok(Json(rows))
}

/// `GET /shiye/api/v1/groups/{id}`
pub async fn get_one(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(id): Path<String>,
) -> AppResult<Json<GroupDetail>> {
    let group = fetch_group(&st, &dev.0, &id).await?;
    let pages = sqlx::query_as::<_, PageBrief>(
        "SELECT id, ord, status, w, h, error FROM pages WHERE group_id = ? ORDER BY ord ASC",
    )
    .bind(&id)
    .fetch_all(&st.pool)
    .await?;
    Ok(Json(GroupDetail { group, pages }))
}

/// `PATCH /shiye/api/v1/groups/{id}`
pub async fn patch(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(id): Path<String>,
    Json(body): Json<PatchGroup>,
) -> AppResult<Json<Group>> {
    fetch_group(&st, &dev.0, &id).await?;

    if let Some(raw) = body.title {
        let title = normalize_title(Some(raw))?;
        sqlx::query("UPDATE groups SET title = ?, updated_at = ? WHERE id = ? AND device_id = ?")
            .bind(&title)
            .bind(db::now())
            .bind(&id)
            .bind(&dev.0)
            .execute(&st.pool)
            .await?;
    }

    if let Some(order) = body.page_order {
        reorder_pages(&st, &id, &order).await?;
    }

    let group = fetch_group(&st, &dev.0, &id).await?;
    Ok(Json(group))
}

/// `DELETE /shiye/api/v1/groups/{id}`
pub async fn delete(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    let affected = sqlx::query("DELETE FROM groups WHERE id = ? AND device_id = ?")
        .bind(&id)
        .bind(&dev.0)
        .execute(&st.pool)
        .await?
        .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound);
    }

    // 数据库级联已删行，这里清磁盘。
    let dir = st.cfg.groups_dir().join(crate::storage::safe_id(&id)?);
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!(error = %e, path = %dir.display(), "清理组目录失败");
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn fetch_group(st: &AppState, device_id: &str, id: &str) -> AppResult<Group> {
    crate::storage::safe_id(id)?;
    sqlx::query_as::<_, Group>(
        "SELECT id, title, state, page_count, done_count, docx_rev, created_at, updated_at
         FROM groups WHERE id = ? AND device_id = ?",
    )
    .bind(id)
    .bind(device_id)
    .fetch_optional(&st.pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// 按给定 id 顺序重排 `pages.ord`，要求 id 集合与库内完全一致。
async fn reorder_pages(st: &AppState, group_id: &str, order: &[String]) -> AppResult<()> {
    let mut existing: Vec<String> =
        sqlx::query_scalar("SELECT id FROM pages WHERE group_id = ? ORDER BY ord ASC")
            .bind(group_id)
            .fetch_all(&st.pool)
            .await?;

    if existing.len() != order.len() {
        return Err(AppError::BadRequest(
            "page_order 数量与当前页数不一致".to_string(),
        ));
    }

    let mut sorted_order = order.to_vec();
    sorted_order.sort_unstable();
    existing.sort_unstable();
    if sorted_order != existing {
        return Err(AppError::BadRequest(
            "page_order 含有不属于该组的页".to_string(),
        ));
    }

    let mut tx = st.pool.begin().await?;
    for (idx, page_id) in order.iter().enumerate() {
        sqlx::query("UPDATE pages SET ord = ? WHERE id = ? AND group_id = ?")
            .bind(idx as i64)
            .bind(page_id)
            .bind(group_id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE groups SET updated_at = ? WHERE id = ?")
        .bind(db::now())
        .bind(group_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_falls_back_to_default() {
        assert_eq!(normalize_title(None).unwrap(), DEFAULT_TITLE);
        assert_eq!(normalize_title(Some("   ".into())).unwrap(), DEFAULT_TITLE);
        assert_eq!(
            normalize_title(Some(" 2026-10-18 11:33 ".into())).unwrap(),
            "2026-10-18 11:33"
        );
    }

    #[test]
    fn title_rejects_overlong() {
        let long = "字".repeat(MAX_TITLE_CHARS + 1);
        assert!(normalize_title(Some(long)).is_err());
    }
}
