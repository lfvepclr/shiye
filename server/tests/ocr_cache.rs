//! 识别缓存：命中时必须完全绕开百度 OCR（假 AK/SK 下也应识别成功）。

use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use image::ImageEncoder;
use serde_json::{json, Value};
use tower::ServiceExt;

use shiye_server::build_app_for_dir;

const DEV: &str = "9f1c2b7e-1111-4222-8333-444455556666";

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 190, 180]));
    let mut cursor = std::io::Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 85);
    enc.write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
    cursor.into_inner()
}

fn multipart(bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----shiye-cache-boundary";
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

fn req(method: &str, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-device-id", DEV)
}

async fn body_json(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
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

async fn poll_until_settled(app: &Router, group_id: &str) -> String {
    let mut state = String::new();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (_, st) = body_json(
            app,
            req("GET", &format!("/shiye/api/v1/groups/{group_id}/status"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        state = st["state"].as_str().unwrap_or_default().to_string();
        if !matches!(state.as_str(), "recognizing" | "queued" | "uploading") {
            break;
        }
    }
    state
}

#[tokio::test]
async fn cache_hit_skips_baidu() {
    let dir = tempfile::tempdir().unwrap();
    let app = build_app_for_dir(dir.path()).await.unwrap();

    let (_, group) = body_json(
        &app,
        req("POST", "/shiye/api/v1/groups")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"缓存命中"}"#))
            .unwrap(),
    )
    .await;
    let group_id = group["id"].as_str().unwrap().to_string();

    let (ctype, body) = multipart(&jpeg(400, 300));
    let resp = app
        .clone()
        .oneshot(
            req("POST", &format!("/shiye/api/v1/groups/{group_id}/pages"))
                .header("content-type", ctype)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let page: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), 4096).await.unwrap()).unwrap();
    let page_id = page["page_id"].as_str().unwrap().to_string();

    // 按页面实际字节（edit.jpg）造缓存：等价于这张图以前被识别过。
    let page_dir = dir.path().join("g").join(&group_id).join(&page_id);
    let edit = std::fs::read(page_dir.join("edit.jpg")).unwrap();
    let key = shiye_server::ocr_cache::key(&edit);
    let cache_dir = dir.path().join("ocr").join(&key);
    std::fs::create_dir_all(&cache_dir).unwrap();
    let entry = json!({
        "version": shiye_server::ocr_cache::VERSION,
        "width": 400,
        "height": 300,
        "blocks": [{
            "ord": 1,
            "label": "text",
            "text": "缓存命中",
            "md": "缓存命中",
            "x1": 10.0,
            "y1": 20.0,
            "x2": 300.0,
            "y2": 60.0,
            "poly": null
        }]
    });
    std::fs::write(
        cache_dir.join("result.json"),
        serde_json::to_vec(&entry).unwrap(),
    )
    .unwrap();

    let (status, _) = body_json(
        &app,
        req("POST", &format!("/shiye/api/v1/groups/{group_id}/render"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 假 AK/SK 调不到百度；能到 done 就证明走的是缓存。
    assert_eq!(poll_until_settled(&app, &group_id).await, "done");

    let (_, share) = body_json(
        &app,
        req("POST", &format!("/shiye/api/v1/groups/{group_id}/share"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    let token = share["token"].as_str().unwrap().to_string();

    let (status, state) = body_json(
        &app,
        Request::builder()
            .uri(format!("/shiye/s/{token}/state"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let blocks = state["blocks"][&page_id]
        .as_array()
        .expect("缓存块应已入库");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["text"], "缓存命中");
}
