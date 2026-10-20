//! 路由装配。所有路由由 `main` 统一挂到 `/shiye` 前缀下。

pub mod groups;
pub mod pages;
pub mod render;
pub mod share;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api: Router<AppState> = Router::new()
        .route("/groups", post(groups::create).get(groups::list))
        .route(
            "/groups/{id}",
            // HarmonyOS 的 `http.RequestMethod` 没有 PATCH，客户端只能发 PUT，两者等价。
            get(groups::get_one)
                .patch(groups::patch)
                .put(groups::patch)
                .delete(groups::delete),
        )
        .route("/groups/{id}/pages", post(pages::upload))
        .route("/pages/{id}", delete(pages::remove).put(pages::replace))
        .route("/pages/{id}/result", get(pages::result))
        .route("/groups/{id}/render", post(render::start))
        .route("/groups/{id}/status", get(render::status))
        .route("/groups/{id}/docx", get(render::download_docx))
        .route("/groups/{id}/share", post(share::create))
        .route("/shares/{token}", delete(share::revoke));

    Router::new()
        .route("/healthz", get(healthz))
        .nest("/api/v1", api)
        .nest("/s", share::viewer_router())
        .with_state(state)
}

/// 探活：不鉴权，顺带验证数据库可用。
async fn healthz(State(st): State<AppState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&st.pool).await {
        Ok(_) => (StatusCode::OK, Json(json!({"ok": true}))),
        Err(e) => {
            tracing::error!(error = %e, "健康检查失败");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"ok": false, "error": "db"})),
            )
        }
    }
}
