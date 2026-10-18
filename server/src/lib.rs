#![forbid(unsafe_code)]
//! 拾页 / Shiye 服务端库：配置、存储、路由与鉴权。
//!
//! 二进制入口见 `src/main.rs`；集成测试可直接调用 [`build_app`]。

pub mod config;
pub mod db;
pub mod device;
pub mod error;
pub mod routes;
pub mod state;
pub mod storage;

use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;

use crate::state::AppState;

/// 组装完整应用：所有路由挂在 `base_path`（默认 `/shiye`）下。
pub fn build_app(state: AppState) -> Router {
    let mount = state.cfg.mount();
    let max_body = state.cfg.max_upload_bytes;
    Router::new()
        .nest(&mount, routes::router(state))
        .layer(RequestBodyLimitLayer::new(max_body))
}

/// 供测试用的内存态应用（临时目录 + SQLite）。
#[cfg(test)]
pub async fn build_test_app() -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("临时目录");
    let cfg = config::Config::default_for_test(dir.path().to_path_buf());
    let pool = db::connect(&cfg.data_dir).await.expect("初始化数据库");
    let state = AppState::new(pool, cfg);
    (build_app(state), dir)
}
