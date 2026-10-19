//! 端到端（进程内）接口测试：建组 → 上传 → 详情 → 重排 → 删页 → 删组。

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use image::ImageEncoder;
use serde_json::{json, Value};
use tower::ServiceExt;

use shiye_server::build_app_for_dir;

/// 建一个临时数据目录下的应用实例。
async fn app() -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let app = build_app_for_dir(dir.path()).await.unwrap();
    (app, dir)
}

const DEV: &str = "9f1c2b7e-1111-4222-8333-444455556666";

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([180, 170, 160]));
    let mut cursor = std::io::Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 85);
    enc.write_image(
        img.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    cursor.into_inner()
}

fn multipart(bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----shiye-test-boundary";
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

async fn body_json(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

fn req(method: &str, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-device-id", DEV)
}

#[tokio::test]
async fn requires_device_id() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::builder().uri("/shiye/api/v1/groups").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn group_and_page_lifecycle() {
    let (app, _dir) = app().await;

    // 建组
    let (status, group) = body_json(
        &app,
        req("POST", "/shiye/api/v1/groups")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"2026-10-18 11:33"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let group_id = group["id"].as_str().unwrap().to_string();
    assert_eq!(group["title"], "2026-10-18 11:33");
    assert_eq!(group["page_count"], 0);

    // 列表
    let (status, list) = body_json(&app, req("GET", "/shiye/api/v1/groups").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);

    // 上传两页
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
    let first: Value = serde_json::from_slice(&to_bytes(resp.into_body(), 1024).await.unwrap()).unwrap();
    let first_id = first["page_id"].as_str().unwrap().to_string();
    assert_eq!(first["ord"], 0);
    assert_eq!(first["w"], 400);

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
    let second: Value = serde_json::from_slice(&to_bytes(resp.into_body(), 1024).await.unwrap()).unwrap();
    let second_id = second["page_id"].as_str().unwrap().to_string();
    assert_eq!(second["ord"], 1);

    // 详情：页数与顺序
    let (status, detail) =
        body_json(&app, req("GET", &format!("/shiye/api/v1/groups/{group_id}")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["page_count"], 2);
    assert_eq!(detail["pages"][0]["id"], first_id);

    // 未识别时下载 docx 应被拒绝
    let resp = app
        .clone()
        .oneshot(
            req("GET", &format!("/shiye/api/v1/groups/{group_id}/docx"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 状态
    let (status, st) = body_json(
        &app,
        req("GET", &format!("/shiye/api/v1/groups/{group_id}/status"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(st["total"], 2);
    assert_eq!(st["done"], 0);
    assert_eq!(st["state"], "draft");
    assert_eq!(st["pages"].as_array().unwrap().len(), 2);

    // 重排：反转页序
    let (status, _) = body_json(
        &app,
        req("PATCH", &format!("/shiye/api/v1/groups/{group_id}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"page_order": [second_id, first_id]}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, detail) =
        body_json(&app, req("GET", &format!("/shiye/api/v1/groups/{group_id}")).body(Body::empty()).unwrap()).await;
    assert_eq!(detail["pages"][0]["id"], second_id);

    // 删页后页序压缩、计数归零
    let resp = app
        .clone()
        .oneshot(
            req("DELETE", &format!("/shiye/api/v1/pages/{second_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let (_, detail) =
        body_json(&app, req("GET", &format!("/shiye/api/v1/groups/{group_id}")).body(Body::empty()).unwrap()).await;
    assert_eq!(detail["page_count"], 1);
    assert_eq!(detail["pages"][0]["id"], first_id);
    assert_eq!(detail["pages"][0]["ord"], 0);

    // 删组
    let resp = app
        .clone()
        .oneshot(
            req("DELETE", &format!("/shiye/api/v1/groups/{group_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let (_, list) = body_json(&app, req("GET", "/shiye/api/v1/groups").body(Body::empty()).unwrap()).await;
    assert!(list.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn render_without_pages_is_rejected() {
    let (app, _dir) = app().await;
    let (_, group) = body_json(
        &app,
        req("POST", "/shiye/api/v1/groups")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    let group_id = group["id"].as_str().unwrap().to_string();
    let resp = app
        .oneshot(
            req("POST", &format!("/shiye/api/v1/groups/{group_id}/render"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn other_device_cannot_see_group() {
    let (app, _dir) = app().await;
    let (_, group) = body_json(
        &app,
        req("POST", "/shiye/api/v1/groups")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    let group_id = group["id"].as_str().unwrap().to_string();

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/shiye/api/v1/groups/{group_id}"))
                .header("x-device-id", "11111111-2222-4333-8444-555555555555")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn share_link_survives_and_can_be_revoked() {
    let (app, _dir) = app().await;
    let (_, group) = body_json(
        &app,
        req("POST", "/shiye/api/v1/groups")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"分享测试"}"#))
            .unwrap(),
    )
    .await;
    let group_id = group["id"].as_str().unwrap().to_string();

    // 生成分享（永久）
    let (status, share) = body_json(
        &app,
        req("POST", &format!("/shiye/api/v1/groups/{group_id}/share"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"expires_in":0}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = share["token"].as_str().unwrap().to_string();
    assert_eq!(token.len(), 22);
    assert!(share["url"]
        .as_str()
        .unwrap()
        .ends_with(&format!("/shiye/s/{token}/")));

    // Viewer 页面：无尾斜杠先 308 到规范地址，相对资源才不会解析到 `/s/state`
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/shiye/s/{token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        resp.headers().get("location").unwrap(),
        &format!("/shiye/s/{token}/").as_str()
    );

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/shiye/s/{token}/"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = to_bytes(resp.into_body(), 512 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&html).contains("拾页"));

    let (status, state) = body_json(
        &app,
        Request::builder()
            .uri(format!("/shiye/s/{token}/state"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["group"]["title"], "分享测试");
    assert!(state["pages"].as_array().unwrap().is_empty());

    // 未授权设备不能撤销
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/shiye/api/v1/shares/{token}"))
                .header("x-device-id", "11111111-2222-4333-8444-555555555555")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 撤销后所有分享入口 404
    let resp = app
        .clone()
        .oneshot(
            req("DELETE", &format!("/shiye/api/v1/shares/{token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/shiye/s/{token}/state"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn replace_page_invalidates_previous_work() {
    let (app, _dir) = app().await;
    // 建组并上传一页
    let (_, group) = body_json(
        &app,
        req("POST", "/shiye/api/v1/groups")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
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
        serde_json::from_slice(&to_bytes(resp.into_body(), 1024).await.unwrap()).unwrap();
    let page_id = page["page_id"].as_str().unwrap().to_string();

    // 换成 600x400
    let (ctype, body) = multipart(&jpeg(600, 400));
    let resp = app
        .clone()
        .oneshot(
            req("PUT", &format!("/shiye/api/v1/pages/{page_id}"))
                .header("content-type", ctype)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let replaced: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), 1024).await.unwrap()).unwrap();
    assert_eq!(replaced["w"], 600);
    assert_eq!(replaced["page_id"], page_id);

    // 组回到 draft，页仍是 1 页
    let (_, detail) =
        body_json(&app, req("GET", &format!("/shiye/api/v1/groups/{group_id}")).body(Body::empty()).unwrap()).await;
    assert_eq!(detail["page_count"], 1);
    assert_eq!(detail["state"], "draft");
    assert_eq!(detail["pages"][0]["w"], 600);

    // 别人的设备不能替换
    let (ctype, body) = multipart(&jpeg(100, 100));
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/shiye/api/v1/pages/{page_id}"))
                .header("x-device-id", "11111111-2222-4333-8444-555555555555")
                .header("content-type", ctype)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
