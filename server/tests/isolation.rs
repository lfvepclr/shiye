//! 多用户隔离：没有登录，设备号即身份，任何跨设备访问都必须 404（不泄露存在性）。

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use image::ImageEncoder;
use serde_json::{json, Value};
use tower::ServiceExt;

use shiye_server::build_app_for_dir;

const DEV_A: &str = "9f1c2b7e-1111-4222-8333-444455556666";
const DEV_B: &str = "0a1b2c3d-2222-4333-8444-555566667777";

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 190, 180]));
    let mut cursor = std::io::Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 85);
    enc.write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
    cursor.into_inner()
}

fn multipart(bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----shiye-isolation-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"page.jpg\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn req(device: &str, method: &str, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-device-id", device)
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let resp = app.clone().oneshot(request).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

fn json_req(device: &str, method: &str, uri: &str, body: &str) -> Request<Body> {
    req(device, method, uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn other_device_cannot_touch_group_or_page() {
    let dir = tempfile::tempdir().unwrap();
    let app = build_app_for_dir(dir.path()).await.unwrap();

    // A 建组 + 上传一页 + 建分享
    let (status, group) = send(
        &app,
        json_req(DEV_A, "POST", "/shiye/api/v1/groups", r#"{"title":"A 的文档"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let gid = group["id"].as_str().unwrap().to_string();

    let (ctype, body) = multipart(&jpeg(400, 300));
    let (status, page) = send(
        &app,
        req(DEV_A, "POST", &format!("/shiye/api/v1/groups/{gid}/pages"))
            .header("content-type", ctype)
            .body(Body::from(body))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let pid = page["page_id"].as_str().unwrap().to_string();

    let (status, share) = send(
        &app,
        json_req(DEV_A, "POST", &format!("/shiye/api/v1/groups/{gid}/share"), "{}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = share["token"].as_str().unwrap().to_string();

    // B 读不到、改不了、删不掉，也看不到 A 的组
    for (method, uri, body) in [
        ("GET", format!("/shiye/api/v1/groups/{gid}"), None),
        ("PATCH", format!("/shiye/api/v1/groups/{gid}"), Some(r#"{"title":"偷改"}"#)),
        ("DELETE", format!("/shiye/api/v1/groups/{gid}"), None),
        ("POST", format!("/shiye/api/v1/groups/{gid}/render"), Some("{}")),
        ("GET", format!("/shiye/api/v1/groups/{gid}/status"), None),
        ("GET", format!("/shiye/api/v1/groups/{gid}/docx"), None),
        ("POST", format!("/shiye/api/v1/groups/{gid}/share"), Some("{}")),
        ("DELETE", format!("/shiye/api/v1/pages/{pid}"), None),
        ("PUT", format!("/shiye/api/v1/pages/{pid}"), None),
        ("DELETE", format!("/shiye/api/v1/shares/{token}"), None),
    ] {
        let request = match body {
            _ if method == "PUT" => {
                let (ctype, bytes) = multipart(&jpeg(400, 300));
                req(DEV_B, method, &uri)
                    .header("content-type", ctype)
                    .body(Body::from(bytes))
                    .unwrap()
            }
            Some(b) => json_req(DEV_B, method, &uri, b),
            None => req(DEV_B, method, &uri).body(Body::empty()).unwrap(),
        };
        let (status, _) = send(&app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri} 应 404");
    }

    // B 的列表里看不到 A 的组
    let (status, list) = send(
        &app,
        req(DEV_B, "GET", "/shiye/api/v1/groups")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 0);

    // A 自己还看得见，分享 token 也没被 B 撤销
    let (status, _) = send(
        &app,
        req(DEV_A, "GET", &format!("/shiye/api/v1/groups/{gid}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, state) = send(
        &app,
        Request::builder()
            .uri(format!("/shiye/s/{token}/state"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "分享页只认 token，不该被越权撤销");
    assert_eq!(state["group"]["title"], "A 的文档");
}

#[tokio::test]
async fn device_id_is_canonicalized() {
    let dir = tempfile::tempdir().unwrap();
    let app = build_app_for_dir(dir.path()).await.unwrap();

    // 大写 UUID 与标准小写视为同一台设备，避免同一客户端分裂出两份数据
    let upper = DEV_A.to_uppercase();
    let (status, _) = send(
        &app,
        json_req(&upper, "POST", "/shiye/api/v1/groups", r#"{"title":"大小写"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, list) = send(
        &app,
        req(DEV_A, "GET", "/shiye/api/v1/groups")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);

    // 非法设备号一律 401，不做任何兜底默认值
    for bad in ["", "not-a-uuid", "9f1c2b7e-1111-4222-8333"] {
        let (status, _) = send(
            &app,
            req(bad, "GET", "/shiye/api/v1/groups")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "设备号 {bad:?} 应被拒绝");
    }
}

#[tokio::test]
async fn unknown_group_and_traversal_ids_are_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let app = build_app_for_dir(dir.path()).await.unwrap();

    // 合法但不存在的 id：404，不能返回 403 让攻击者区分「存在但无权」
    let (status, _) = send(
        &app,
        req(DEV_A, "GET", "/shiye/api/v1/groups/00000000-0000-4000-8000-000000000000")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 路径穿越一律拒绝（400 或 404，不能 5xx）
    let (status, _) = send(
        &app,
        req(DEV_A, "GET", "/shiye/api/v1/groups/..%2f..%2fetc")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
        "穿越路径应被拒绝，实际 {status}"
    );
}
