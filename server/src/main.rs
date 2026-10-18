#![forbid(unsafe_code)]
//! 拾页 / Shiye 服务端入口。

use std::path::PathBuf;

use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use shiye_server::config::Config;
use shiye_server::state::AppState;
use shiye_server::{build_app, db};

fn parse_args() -> PathBuf {
    let mut path = std::env::var("SHIYE_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if (arg == "--config" || arg == "-c") && args.size_hint().0 > 0 {
            if let Some(next) = args.next() {
                path = next;
            }
        }
    }
    PathBuf::from(path)
}

#[tokio::main(worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,axum=warn")),
        )
        .init();

    let cfg_path = parse_args();
    let cfg = Config::load(&cfg_path)?;
    tracing::info!(
        config = %cfg_path.display(),
        base_path = %cfg.mount(),
        data_dir = %cfg.data_dir.display(),
        "配置已加载"
    );

    std::fs::create_dir_all(&cfg.data_dir)?;
    let pool = db::connect(&cfg.data_dir).await?;
    let max_body = cfg.max_upload_bytes;
    let state = AppState::new(pool, cfg.clone());

    let app = build_app(state)
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(max_body));

    let listener = TcpListener::bind(&cfg.listen).await?;
    tracing::info!(addr = %cfg.listen, "拾页服务已启动");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("已停止");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "无法注册 SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("收到 SIGINT"),
        _ = terminate => tracing::info!("收到 SIGTERM"),
    }
}
