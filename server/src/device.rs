//! `X-Device-Id` 提取器：无账号体系，设备即归属。

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::error::AppError;

/// 已校验的 UUIDv4 设备标识。
pub struct DeviceId(pub String);

impl<S> FromRequestParts<S> for DeviceId
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get("x-device-id")
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::Unauthorized)?;

        let parsed = Uuid::parse_str(raw.trim()).map_err(|_| AppError::Unauthorized)?;
        Ok(DeviceId(parsed.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, Request};

    async fn extract(headers: HeaderMap) -> Result<String, AppError> {
        let mut req = Request::builder().body(()).unwrap();
        *req.headers_mut() = headers;
        let (mut parts, _) = req.into_parts();
        DeviceId::from_request_parts(&mut parts, &()).await.map(|d| d.0)
    }

    #[tokio::test]
    async fn accepts_uuid() {
        let mut h = HeaderMap::new();
        h.insert("x-device-id", "9f1c2b7e-1111-4222-8333-444455556666".parse().unwrap());
        assert_eq!(
            extract(h).await.unwrap(),
            "9f1c2b7e-1111-4222-8333-444455556666"
        );
    }

    #[tokio::test]
    async fn rejects_garbage() {
        let mut h = HeaderMap::new();
        h.insert("x-device-id", "not-a-uuid".parse().unwrap());
        assert!(extract(h).await.is_err());
    }

    #[tokio::test]
    async fn rejects_missing() {
        assert!(extract(HeaderMap::new()).await.is_err());
    }
}
