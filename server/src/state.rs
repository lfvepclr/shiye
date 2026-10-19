//! 共享应用状态。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use sqlx::SqlitePool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: Arc<Config>,
    pub http: reqwest::Client,
    /// 正在识别中的组（进程内去重，防止重复触发 render）。
    pub rendering: Arc<Mutex<HashSet<String>>>,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: Config) -> Self {
        Self {
            pool,
            cfg: Arc::new(cfg),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .expect("构建 HTTP 客户端"),
            rendering: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}
