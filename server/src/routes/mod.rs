//! 路由装配。所有路由由 `main` 统一挂到 `/shiye` 前缀下。

pub mod groups;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api: Router<AppState> = Router::new()
        .route("/groups", post(groups::create).get(groups::list))
        .route(
            "/groups/{id}",
            get(groups::get_one)
                .patch(groups::patch)
                .delete(groups::delete),
        );

    Router::new()
        .route("/healthz", get(healthz))
        .nest("/api/v1", api)
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
