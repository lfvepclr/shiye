#![forbid(unsafe_code)]
//! 拾页 / Shiye 服务端库：配置、存储、路由与鉴权。
//!
//! 二进制入口见 `src/main.rs`；集成测试可直接调用 [`build_app`]。

pub mod baidu;
pub mod blocks;
pub mod config;
pub mod db;
pub mod device;
pub mod docx;
pub mod error;
pub mod imaging;
pub mod latex;
pub mod ocr_cache;
pub mod render;
pub mod routes;
pub mod state;
pub mod storage;

use axum::extract::DefaultBodyLimit;
use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;

use crate::state::AppState;

/// 组装完整应用：所有路由挂在 `base_path`（默认 `/shiye`）下。
pub fn build_app(state: AppState) -> Router {
    let mount = state.cfg.mount();
    let max_body = state.cfg.max_upload_bytes;
    Router::new()
        .nest(&mount, routes::router(state))
        // axum 的 DefaultBodyLimit 默认只有 2MB，高清照片会在这里被截断，
        // 表现为 multipart 解析失败而不是 413。
        .layer(DefaultBodyLimit::max(max_body))
        .layer(RequestBodyLimitLayer::new(max_body))
}

/// 供测试用的应用实例：数据目录由调用方提供（生产入口不走这里）。
#[doc(hidden)]
pub async fn build_app_for_dir(dir: &std::path::Path) -> anyhow::Result<Router> {
    let cfg = config::Config::for_test(dir.to_path_buf());
    let pool = db::connect(&cfg.data_dir).await?;
    Ok(build_app(AppState::new(pool, cfg)))
}
