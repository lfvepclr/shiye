//! 百度智能云 PaddleOCR-VL 文档解析客户端（异步任务接口）。
//!
//! 链路：取 access_token（30 天缓存）→ 提交任务 → 轮询 → 下载结果 JSON。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::AppError;

const TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";
const SUBMIT_URL: &str =
    "https://aip.baidubce.com/rest/2.0/brain/online/v2/paddle-vl-parser/task";
const QUERY_URL: &str = "https://aip.baidubce.com/rest/2.0/brain/online/v2/paddle-vl-parser/task/query";

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct BaiduClient<'a> {
    http: &'a reqwest::Client,
    api_key: &'a str,
    secret_key: &'a str,
    pool: &'a SqlitePool,
    poll_interval: Duration,
    timeout: Duration,
}

impl<'a> BaiduClient<'a> {
    pub fn new(
        http: &'a reqwest::Client,
        api_key: &'a str,
        secret_key: &'a str,
        pool: &'a SqlitePool,
        poll_interval_secs: u64,
        timeout_secs: u64,
    ) -> Self {
        Self {
            http,
            api_key,
            secret_key,
            pool,
            poll_interval: Duration::from_secs(poll_interval_secs.max(1)),
            timeout: Duration::from_secs(timeout_secs.max(30)),
        }
    }

    /// 提交单页图片并等待结果，返回该页的百度 JSON。
    pub async fn parse_page(&self, jpeg: &[u8], file_name: &str) -> Result<Value, AppError> {
        let token = self.access_token().await?;
        let file_data = base64::engine::general_purpose::STANDARD.encode(jpeg);

        let submit = self
            .post_form(
                SUBMIT_URL,
                &token,
                &[
                    ("file_data", file_data.as_str()),
                    ("file_name", file_name),
                    ("analysis_chart", "true"),
                    ("merge_tables", "true"),
                    ("relevel_titles", "true"),
                    ("recognize_seal", "true"),
                ],
            )
            .await?;
        let task_id = submit
            .pointer("/result/task_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::internal(format!("百度未返回 task_id: {}", truncate(&submit)))
            })?;

        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(AppError::internal(format!(
                    "百度解析超时（task {task_id}）"
                )));
            }
            tokio::time::sleep(self.poll_interval).await;
            let queried = self
                .post_form(QUERY_URL, &token, &[("task_id", task_id)])
                .await?;
            let result = queried.get("result").cloned().unwrap_or(Value::Null);
            let status = result
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            match status.as_str() {
                "success" => {
                    let url = result
                        .get("parse_result_url")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            AppError::internal("百度解析成功但未返回结果链接".to_string())
                        })?;
                    return self.download_result(url).await;
                }
                "failed" | "error" => {
                    let msg = result
                        .get("task_error")
                        .and_then(Value::as_str)
                        .or(queried.get("error_msg").and_then(Value::as_str))
                        .unwrap_or("未知错误");
                    return Err(AppError::internal(format!("百度解析失败: {msg}")));
                }
                _ => tracing::debug!(task = task_id, status = %status, "百度任务进行中"),
            }
        }
    }

    async fn download_result(&self, url: &str) -> Result<Value, AppError> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("下载百度结果失败: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::internal(format!(
                "下载百度结果失败: HTTP {}",
                resp.status()
            )));
        }
        let json: Value = resp
            .json()
            .await
            .map_err(|e| AppError::internal(format!("解析百度结果 JSON 失败: {e}")))?;
        Ok(json)
    }

    async fn post_form(
        &self,
        url: &str,
        token: &str,
        fields: &[(&str, &str)],
    ) -> Result<Value, AppError> {
        let resp = self
            .http
            .post(url)
            .query(&[("access_token", token)])
            .form(fields)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("百度接口请求失败: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AppError::internal(format!("读取百度响应失败: {e}")))?;
        let json: Value = serde_json::from_str(&text).map_err(|_| {
            AppError::internal(format!(
                "百度响应非 JSON: {}",
                text.chars().take(200).collect::<String>()
            ))
        })?;
        if let Some(code) = json.get("error_code").and_then(Value::as_i64) {
            if code != 0 {
                return Err(AppError::internal(format!(
                    "百度接口错误 {code}: {}",
                    json.get("error_msg").and_then(Value::as_str).unwrap_or("")
                )));
            }
        }
        if !status.is_success() {
            return Err(AppError::internal(format!("百度接口 HTTP {status}")));
        }
        Ok(json)
    }

    /// access_token：30 天有效，提前 1 天续期，落库重启不丢。
    async fn access_token(&self) -> Result<String, AppError> {
        if self.api_key.is_empty() || self.secret_key.is_empty() {
            return Err(AppError::internal("百度 API Key / Secret Key 未配置"));
        }
        let now = now_ts();
        let stored: Option<(String, i64)> = sqlx::query_as(
            "SELECT (SELECT v FROM settings WHERE k='baidu.access_token'),
                    CAST((SELECT v FROM settings WHERE k='baidu.token_expire_at') AS INTEGER)",
        )
        .fetch_optional(self.pool)
        .await
        .unwrap_or(None);

        if let Some((token, expire_at)) = stored {
            if !token.is_empty() && expire_at > (now + 86400) as i64 {
                return Ok(token);
            }
        }

        let resp = self
            .http
            .post(TOKEN_URL)
            .query(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.api_key),
                ("client_secret", self.secret_key),
            ])
            .send()
            .await
            .map_err(|e| AppError::internal(format!("获取百度 token 失败: {e}")))?;
        let json: Value = resp
            .json()
            .await
            .map_err(|e| AppError::internal(format!("解析百度 token 响应失败: {e}")))?;
        let token = json
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::internal(format!(
                    "获取百度 token 失败: {}",
                    json.get("error_description")
                        .and_then(Value::as_str)
                        .unwrap_or("未知错误")
                ))
            })?
            .to_string();
        let expires_in = json.get("expires_in").and_then(Value::as_i64).unwrap_or(2592000);
        let expire_at = (now + expires_in as u64).saturating_sub(86400) as i64;

        for (k, v) in [
            ("baidu.access_token", token.clone()),
            ("baidu.token_expire_at", expire_at.to_string()),
        ] {
            sqlx::query(
                "INSERT INTO settings (k, v) VALUES (?, ?)
                 ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            )
            .bind(k)
            .bind(v)
            .execute(self.pool)
            .await
            .map_err(AppError::internal)?;
        }
        Ok(token)
    }
}

fn truncate(v: &Value) -> String {
    let s = v.to_string();
    s.chars().take(200).collect()
}
