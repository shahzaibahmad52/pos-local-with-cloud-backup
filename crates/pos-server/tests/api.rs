//! The HTTP API as a till uses it: sign in, open a shift, sell, print.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use pos_server::{db, router, seed, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

fn app() -> Router {
    let mut conn = db::open_in_memory().unwrap();
    seed::demo(&mut conn).unwrap();
    router(AppState::new(conn))
}

async fn call(app: &Router, method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
    (status, value)
}

#[tokio::test]
async fn till_flow() {
    let app = app();

    let (status, health) = call(&app, "GET", "/api/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["setUp"], true);
    assert_eq!(health["store"]["code"], "B1");

    // Everything else needs a session.
    let (status, body) = call(&app, "GET", "/api/products/search?q=pan", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");

    let (_, tiles) = call(&app, "GET", "/api/login-tiles", None, None).await;
    let ali =
        tiles.as_array().unwrap().iter().find(|t| t["name"] == "Ali").unwrap()["id"].as_str().unwrap().to_string();

    let (status, body) = call(&app, "POST", "/api/login", None, Some(json!({ "userId": ali, "secret": "9999" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, login) =
        call(&app, "POST", "/api/login", None, Some(json!({ "userId": ali, "secret": "1234" }))).await;
    assert_eq!(status, StatusCode::OK, "{login}");
    let token = login["token"].as_str().unwrap().to_string();
    let t = Some(token.as_str());

    let (status, shift) =
        call(&app, "POST", "/api/shifts/open", t, Some(json!({ "tillCode": "T01", "openingCash": 500000 }))).await;
    assert_eq!(status, StatusCode::OK, "{shift}");

    let (_, found) = call(&app, "GET", "/api/products/barcode/8964000123463", t, None).await;
    assert_eq!(found["product"]["name"], "Panadol 500mg Tablet");
    let strip = found["unitId"].as_str().unwrap().to_string();
    let product_id = found["product"]["id"].as_str().unwrap().to_string();

    let sale = json!({
        "tillCode": "T01",
        "lines": [{ "productId": product_id, "unitId": strip, "qty": 2 }],
        "payments": [{ "method": "cash", "amount": 10000 }],
        "clientRef": "test-1"
    });
    let (status, receipt) = call(&app, "POST", "/api/sales", t, Some(sale)).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    assert_eq!(receipt["receiptNo"], "B1-T01-000001");
    assert_eq!(receipt["total"], 6000);
    assert_eq!(receipt["changeDue"], 4000);
    assert_eq!(receipt["cashierName"], "Ali");
    assert_eq!(receipt["store"]["name"], "Branch 1 — Gulberg");

    let id = receipt["id"].as_str().unwrap();
    let (status, again) = call(&app, "GET", &format!("/api/sales/{id}/receipt"), t, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["receiptNo"], "B1-T01-000001");

    // Cashiers can't add products.
    let (status, _) =
        call(&app, "POST", "/api/products", t, Some(json!({ "name": "X", "kind": "general", "baseUnit": "pc" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(&app, "PATCH", "/api/me/language", t, Some(json!({ "language": "ur" }))).await;
    assert_eq!(status, StatusCode::OK);
    let (_, me) = call(&app, "GET", "/api/me", t, None).await;
    assert_eq!(me["language"], "ur");

    call(&app, "POST", "/api/logout", t, None).await;
    let (status, _) = call(&app, "GET", "/api/me", t, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
