//! 分享链接（客户端接口）与只读 Web Viewer（分享页接口）。
//!
//! 分享页的一切前端请求都走相对路径（`./state`、`./img/…`），因此 `base_path`
//! 或反代前缀怎么改都不会拼错。

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::blocks::{sanitize_fig_name, sanitize_html, strip_html};
use crate::db;
use crate::device::DeviceId;
use crate::error::{AppError, AppResult};
use crate::latex;
use crate::routes::groups;
use crate::state::AppState;
use crate::storage;

/// 内嵌的单页 Viewer（无框架、无 CDN）。
const VIEWER_HTML: &str = include_str!("../../viewer/index.html");

/// token 长度：22 个十六进制字符 = 88 bit 随机量。
const TOKEN_LEN: usize = 22;

#[derive(Debug, Serialize)]
pub struct ShareCreated {
    pub token: String,
    pub url: String,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateShare {
    /// 有效期（秒）。缺省取配置里的 `share_ttl_days`；`<= 0` 表示永久。
    #[serde(default)]
    pub expires_in: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct ShareRow {
    group_id: String,
    expires_at: Option<i64>,
    revoked: i64,
}

// ---------------------------------------------------------------------------
// 客户端接口
// ---------------------------------------------------------------------------

/// `POST /shiye/api/v1/groups/{id}/share`
pub async fn create(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(group_id): Path<String>,
    Json(body): Json<CreateShare>,
) -> AppResult<Json<ShareCreated>> {
    groups::fetch_group(&st, &dev.0, &group_id).await?;

    let expires_at = match body.expires_in {
        Some(secs) if secs <= 0 => None,
        Some(secs) => Some(db::now() + secs),
        None => Some(db::now() + st.cfg.share_ttl_days.max(1) * 86_400),
    };

    let token = new_token();
    sqlx::query(
        "INSERT INTO shares (token, group_id, expires_at, revoked, created_at)
         VALUES (?, ?, ?, 0, ?)",
    )
    .bind(&token)
    .bind(&group_id)
    .bind(expires_at)
    .bind(db::now())
    .execute(&st.pool)
    .await?;

    Ok(Json(ShareCreated {
        url: share_url(&st, &token),
        token,
        expires_at,
    }))
}

/// `DELETE /shiye/api/v1/shares/{token}`
pub async fn revoke(
    State(st): State<AppState>,
    dev: DeviceId,
    Path(token): Path<String>,
) -> AppResult<StatusCode> {
    let affected = sqlx::query(
        "UPDATE shares SET revoked = 1
         WHERE token = ? AND group_id IN (SELECT id FROM groups WHERE device_id = ?)",
    )
    .bind(&token)
    .bind(&dev.0)
    .execute(&st.pool)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Viewer
// ---------------------------------------------------------------------------

/// `GET /shiye/s/{token}`
///
/// Viewer 内部全部用相对地址（`./state`、`./img/…`）。浏览器解析相对地址时
/// 会把最后一段当成文件名，`/s/<token>` 就会解析成 `/s/state` 而 404，
/// 所以这里统一跳到带尾斜杠的规范地址。
pub async fn page(
    State(st): State<AppState>,
    Path(token): Path<String>,
    uri: OriginalUri,
) -> AppResult<Response> {
    resolve(&st, &token).await?;
    let path = uri.path();
    if !path.ends_with('/') {
        let target = format!("{path}/");
        let resp = Response::builder()
            .status(StatusCode::PERMANENT_REDIRECT)
            .header(header::LOCATION, target)
            .body(Body::empty())
            .map_err(AppError::internal)?;
        return Ok(resp);
    }
    Ok(Html(VIEWER_HTML).into_response())
}

#[derive(Debug, Serialize)]
pub struct ViewerGroup {
    title: String,
    state: String,
    page_count: i64,
    created_at: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ViewerPage {
    id: String,
    ord: i64,
    w: i64,
    h: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ViewerBlock {
    #[serde(skip)]
    page_id: String,
    ord: i64,
    label: String,
    text: String,
    md: String,
    /// 已转成纯文本（行内公式 → Unicode），网页叠加层直接铺在原图上。
    #[sqlx(default)]
    plain: String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

#[derive(Debug, Serialize)]
pub struct ViewerState {
    group: ViewerGroup,
    pages: Vec<ViewerPage>,
    blocks: HashMap<String, Vec<ViewerBlock>>,
}

/// `GET /shiye/s/{token}/state`
pub async fn state(
    State(st): State<AppState>,
    Path(token): Path<String>,
) -> AppResult<Json<ViewerState>> {
    let share = resolve(&st, &token).await?;

    let group = sqlx::query_as::<_, (String, String, i64, i64)>(
        "SELECT title, state, page_count, created_at FROM groups WHERE id = ?",
    )
    .bind(&share.group_id)
    .fetch_optional(&st.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let pages = sqlx::query_as::<_, ViewerPage>(
        "SELECT id, ord, w, h FROM pages WHERE group_id = ? AND status = 'done' ORDER BY ord ASC",
    )
    .bind(&share.group_id)
    .fetch_all(&st.pool)
    .await?;

    let mut rows = sqlx::query_as::<_, ViewerBlock>(
        "SELECT b.page_id, b.ord, b.label, b.text, b.md, b.x1, b.y1, b.x2, b.y2
         FROM blocks b JOIN pages p ON p.id = b.page_id
         WHERE p.group_id = ? AND p.status = 'done'
         ORDER BY p.ord ASC, b.ord ASC",
    )
    .bind(&share.group_id)
    .fetch_all(&st.pool)
    .await?;

    for row in rows.iter_mut() {
        row.plain = crate::blocks::plain_text(&row.md);
    }

    let mut blocks: HashMap<String, Vec<ViewerBlock>> = HashMap::new();
    for row in rows {
        blocks.entry(row.page_id.clone()).or_default().push(row);
    }

    Ok(Json(ViewerState {
        group: ViewerGroup {
            title: group.0,
            state: group.1,
            page_count: group.2,
            created_at: group.3,
        },
        pages,
        blocks,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ImageQuery {
    #[serde(default)]
    pub v: Option<String>,
}

/// `GET /shiye/s/{token}/img/{page_id}?v=thumb|disp`
pub async fn image(
    State(st): State<AppState>,
    Path((token, page_id)): Path<(String, String)>,
    Query(q): Query<ImageQuery>,
) -> AppResult<Response> {
    let share = resolve(&st, &token).await?;
    ensure_page_owner(&st, &share.group_id, &page_id).await?;
    let name = if q.v.as_deref() == Some("disp") {
        "disp.jpg"
    } else {
        "thumb.jpg"
    };
    let path = storage::preview_file(&st.cfg.data_dir, &share.group_id, &page_id, name);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound)?;
    Ok(immutable(bytes, "image/jpeg"))
}

/// `GET /shiye/s/{token}/fig/{page_id}/{name}`（OCR 插图）
pub async fn figure(
    State(st): State<AppState>,
    Path((token, page_id, name)): Path<(String, String, String)>,
) -> AppResult<Response> {
    let share = resolve(&st, &token).await?;
    ensure_page_owner(&st, &share.group_id, &page_id).await?;
    let safe = sanitize_fig_name(&name).ok_or(AppError::NotFound)?;
    let path = storage::fig_file(&st.cfg.data_dir, &share.group_id, &page_id, &safe);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound)?;
    let mime = if safe.ends_with(".png") {
        "image/png"
    } else {
        "image/jpeg"
    };
    Ok(immutable(bytes, mime))
}

#[derive(Debug, Deserialize)]
pub struct RichQuery {
    /// 逗号分隔的块序号（`ord`）；缺省返回整页。
    #[serde(default)]
    pub blocks: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RichText {
    pub html: String,
    pub text: String,
}

/// `GET /shiye/s/{token}/richtext/{page_id}?blocks=1,5,7`
pub async fn richtext(
    State(st): State<AppState>,
    Path((token, page_id)): Path<(String, String)>,
    Query(q): Query<RichQuery>,
) -> AppResult<Json<RichText>> {
    let share = resolve(&st, &token).await?;
    ensure_page_owner(&st, &share.group_id, &page_id).await?;

    let rows = sqlx::query_as::<_, ViewerBlock>(
        "SELECT page_id, ord, label, text, md, x1, y1, x2, y2
         FROM blocks WHERE page_id = ? ORDER BY ord ASC",
    )
    .bind(&page_id)
    .fetch_all(&st.pool)
    .await?;

    let wanted: Option<Vec<i64>> = q.blocks.as_deref().map(|raw| {
        raw.split(',')
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .collect()
    });

    let base = share_asset_base(&st, &token, &page_id);
    let mut html = String::new();
    let mut text = String::new();
    for row in rows {
        if let Some(list) = &wanted {
            if !list.contains(&row.ord) {
                continue;
            }
        }
        html.push_str(&block_to_html(&row, &base));
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(strip_html(&row.text).trim());
    }

    Ok(Json(RichText { html, text }))
}

fn block_to_html(block: &ViewerBlock, base: &str) -> String {
    let md = block.md.trim();
    if md.is_empty() {
        return String::new();
    }
    let heading = md.chars().take_while(|c| *c == '#').count();
    if heading > 0 && heading <= 6 {
        let body = md.trim_start_matches('#').trim();
        return format!("<h{heading}>{body}</h{heading}>\n");
    }
    if md.starts_with("<table") {
        let clean = sanitize_html(md);
        return format!("{}\n", absolutize_img(&clean, base));
    }
    if md.starts_with("$$") {
        return format!("<p>{}</p>\n", latex::to_unicode(md));
    }
    let body = md
        .replace("<br/>", "<br>")
        .replace("<br />", "<br>");
    format!("<p>{}</p>\n", absolutize_img(&body, base))
}

/// Viewer 文本里的 `<img src="fig_x.png">` 改写成绝对地址，Word 才能显示。
fn absolutize_img(html: &str, base: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find("<img") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        let tag = &tail[..=end];
        rest = &tail[end + 1..];
        if let Some(name) = crate::blocks::sanitize_fig_name(&attr_value(tag, "src").unwrap_or_default()) {
            out.push_str(&format!("<img src=\"{base}/{name}\" alt=\"\">"));
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

/// 插图绝对地址前缀：`{public_base_url}/{base_path}/s/{token}/fig/{page_id}`。
fn share_asset_base(st: &AppState, token: &str, page_id: &str) -> String {
    let prefix = if st.cfg.public_base_url.is_empty() {
        String::new()
    } else {
        st.cfg.public_base_url.clone()
    };
    format!(
        "{}{}/s/{token}/fig/{page_id}",
        prefix,
        st.cfg.mount()
    )
}

// ---------------------------------------------------------------------------
// 公共辅助
// ---------------------------------------------------------------------------

async fn resolve(st: &AppState, token: &str) -> AppResult<ShareRow> {
    storage::safe_id(token)?;
    let share = sqlx::query_as::<_, ShareRow>(
        "SELECT group_id, expires_at, revoked FROM shares WHERE token = ?",
    )
    .bind(token)
    .fetch_optional(&st.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    if share.revoked != 0 {
        return Err(AppError::NotFound);
    }
    if let Some(exp) = share.expires_at {
        if exp <= db::now() {
            return Err(AppError::NotFound);
        }
    }
    Ok(share)
}

async fn ensure_page_owner(st: &AppState, group_id: &str, page_id: &str) -> AppResult<()> {
    storage::safe_id(page_id)?;
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM pages WHERE id = ? AND group_id = ?")
            .bind(page_id)
            .bind(group_id)
            .fetch_optional(&st.pool)
            .await?;
    exists.map(|_| ()).ok_or(AppError::NotFound)
}

fn immutable(bytes: Vec<u8>, mime: &'static str) -> Response {
    let mut resp = Response::new(Body::from(bytes));
    let headers = resp.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    resp
}

fn new_token() -> String {
    let a = uuid::Uuid::new_v4().simple().to_string();
    let b = uuid::Uuid::new_v4().simple().to_string();
    format!("{a}{b}").chars().take(TOKEN_LEN).collect()
}

fn share_url(st: &AppState, token: &str) -> String {
    build_share_url(&st.cfg.public_base_url, &st.cfg.mount(), token)
}

fn build_share_url(public_base: &str, mount: &str, token: &str) -> String {
    // 带尾斜杠是规范地址：Viewer 内部资源全部是相对路径，少了斜杠会被解析到 `/s/…`
    if public_base.is_empty() {
        format!("{mount}/s/{token}/")
    } else {
        format!("{public_base}{mount}/s/{token}/")
    }
}

/// Viewer 路由（挂在 `/shiye/s` 下，全部只读）。
pub fn viewer_router() -> axum::Router<AppState> {
    use axum::routing::get;
    axum::Router::new()
        .route("/{token}", get(page))
        // 规范地址（带尾斜杠）；相对资源要靠它才不会解析到 `/s/state`
        .route("/{token}/", get(page))
        .route("/{token}/state", get(state))
        .route("/{token}/img/{page_id}", get(image))
        .route("/{token}/fig/{page_id}/{name}", get(figure))
        .route("/{token}/richtext/{page_id}", get(richtext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_22_hex_chars() {
        let t = new_token();
        assert_eq!(t.len(), TOKEN_LEN);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(t, new_token());
    }

    #[test]
    fn html_absolutizes_fig() {
        let html = absolutize_img("<img src=\"fig_ab12.png\">", "https://x/shiye/s/t/fig/p1");
        assert!(html.contains("https://x/shiye/s/t/fig/p1/fig_ab12.png"));
    }

    #[test]
    fn richtext_heading() {
        let block = ViewerBlock {
            page_id: "p".into(),
            ord: 1,
            label: "paragraph_title".into(),
            text: "标题".into(),
            md: "# 标题".into(),
            plain: "标题".into(),
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        };
        assert_eq!(block_to_html(&block, "base"), "<h1>标题</h1>\n");
    }

    #[test]
    fn share_url_uses_public_base() {
        assert_eq!(
            build_share_url("https://example.com", "/shiye", "abc"),
            "https://example.com/shiye/s/abc/"
        );
        assert_eq!(build_share_url("", "/shiye", "abc"), "/shiye/s/abc/");
    }

}
