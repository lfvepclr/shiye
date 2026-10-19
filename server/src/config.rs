//! 配置加载：`config.toml` → `Config`。

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,

    /// 统一前缀，默认 `shiye` → 所有路由挂在 `/shiye` 下。
    #[serde(default = "default_base_path")]
    pub base_path: String,

    /// 生成分享链接用的外部基址，例如 `https://example.com`。
    #[serde(default)]
    pub public_base_url: String,

    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    #[serde(default = "default_share_ttl")]
    pub share_ttl_days: i64,

    #[serde(default = "default_max_upload")]
    pub max_upload_bytes: usize,

    #[serde(default = "default_max_pixels")]
    pub max_image_pixels: u64,

    #[serde(default = "default_display_edge")]
    pub display_max_edge: u32,

    #[serde(default = "default_thumb_edge")]
    pub thumb_max_edge: u32,

    #[serde(default = "default_ocr_concurrency")]
    pub ocr_concurrency: usize,

    #[serde(default = "default_poll_interval")]
    pub ocr_poll_interval_secs: u64,

    #[serde(default = "default_ocr_timeout")]
    pub ocr_timeout_secs: u64,

    #[serde(default)]
    pub baidu: BaiduConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BaiduConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub secret_key: String,
}

fn default_listen() -> String {
    "127.0.0.1:8080".to_string()
}
fn default_base_path() -> String {
    "shiye".to_string()
}
fn default_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/shiye")
}
fn default_share_ttl() -> i64 {
    30
}
fn default_max_upload() -> usize {
    6 * 1024 * 1024
}
fn default_max_pixels() -> u64 {
    25_000_000
}
fn default_display_edge() -> u32 {
    1600
}
fn default_thumb_edge() -> u32 {
    320
}
fn default_ocr_concurrency() -> usize {
    2
}
fn default_poll_interval() -> u64 {
    3
}
fn default_ocr_timeout() -> u64 {
    1800
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("读取配置 {} 失败: {e}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&raw).map_err(|e| anyhow::anyhow!("解析配置 {} 失败: {e}", path.display()))?;
        cfg.base_path = cfg.base_path.trim_matches('/').to_string();
        if cfg.base_path.is_empty() {
            cfg.base_path = default_base_path();
        }
        cfg.public_base_url = cfg.public_base_url.trim_end_matches('/').to_string();
        Ok(cfg)
    }

    /// 挂载点，形如 `/shiye`。
    pub fn mount(&self) -> String {
        format!("/{}", self.base_path)
    }

    pub fn db_url(&self) -> String {
        format!("sqlite://{}/shiye.db?mode=rwc", self.data_dir.display())
    }

    pub fn groups_dir(&self) -> PathBuf {
        self.data_dir.join("g")
    }

    /// 测试用配置：数据目录指向临时目录。生产路径不会调用。
    #[doc(hidden)]
    pub fn for_test(data_dir: PathBuf) -> Self {
        Self {
            listen: default_listen(),
            base_path: default_base_path(),
            public_base_url: "https://example.invalid".to_string(),
            data_dir,
            share_ttl_days: default_share_ttl(),
            max_upload_bytes: default_max_upload(),
            max_image_pixels: default_max_pixels(),
            display_max_edge: default_display_edge(),
            thumb_max_edge: default_thumb_edge(),
            ocr_concurrency: default_ocr_concurrency(),
            ocr_poll_interval_secs: default_poll_interval(),
            ocr_timeout_secs: default_ocr_timeout(),
            baidu: BaiduConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_path_is_normalized() {
        let cfg: Config = toml::from_str("base_path = \"/app/\"\n").unwrap();
        let mut cfg = cfg;
        cfg.base_path = cfg.base_path.trim_matches('/').to_string();
        assert_eq!(cfg.mount(), "/app");
    }

    #[test]
    fn defaults_are_sane() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.listen, "127.0.0.1:8080");
        assert_eq!(cfg.base_path, "shiye");
        assert_eq!(cfg.share_ttl_days, 30);
    }
}
