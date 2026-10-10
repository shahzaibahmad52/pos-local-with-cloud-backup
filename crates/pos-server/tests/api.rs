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

#[tokio::test]
async fn errors_carry_reason_and_params_for_translation() {
    let app = app();
    let (_, tiles) = call(&app, "GET", "/api/login-tiles", None, None).await;
    let id_of = |name: &str| {
        tiles.as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["id"].as_str().unwrap().to_string()
    };
    let (_, login) =
        call(&app, "POST", "/api/login", None, Some(json!({ "userId": id_of("Ali"), "secret": "1234" }))).await;
    let token = login["token"].as_str().unwrap().to_string();
    let t = Some(token.as_str());

    let (_, found) = call(&app, "GET", "/api/products/search?q=flagyl", t, None).await;
    let flagyl = &found[0];
    let strip = flagyl["units"].as_array().unwrap().iter().find(|u| u["name"] == "Strip").unwrap()["id"].clone();
    let sale = json!({
        "tillCode": "T01",
        "lines": [{ "productId": flagyl["id"], "unitId": strip, "qty": 13 }],
        "payments": [{ "method": "cash", "amount": 100000 }]
    });
    let (status, body) = call(&app, "POST", "/api/sales", t, Some(sale)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "insufficient_stock");
    assert_eq!(body["reason"], "stock.short");
    assert_eq!(body["params"]["have"], 120);
    assert_eq!(body["params"]["expired"], 50);

    // Cashiers can read the rules but not the activity log.
    let (status, rules) = call(&app, "GET", "/api/rules", t, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rules["cashierMaxDiscountBp"], 500);
    let (status, _) = call(&app, "GET", "/api/activity?date=2026-10-10", t, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, owner) =
        call(&app, "POST", "/api/login", None, Some(json!({ "userId": id_of("Owner"), "secret": "owner1234" }))).await;
    let owner_token = owner["token"].as_str().unwrap().to_string();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let (status, log) =
        call(&app, "GET", &format!("/api/activity?date={today}"), Some(owner_token.as_str()), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(log.as_array().unwrap().iter().any(|e| e["action"] == "store.setup"));
}

#[tokio::test]
async fn backoffice_is_for_admins_only() {
    let app = app();
    let (_, tiles) = call(&app, "GET", "/api/login-tiles", None, None).await;
    let id_of = |name: &str| {
        tiles.as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["id"].as_str().unwrap().to_string()
    };
    let sign_in = |id: String, secret: &'static str| {
        let app = app.clone();
        async move {
            let (_, body) =
                call(&app, "POST", "/api/login", None, Some(json!({ "userId": id, "secret": secret }))).await;
            body["token"].as_str().unwrap().to_string()
        }
    };
    let ali = sign_in(id_of("Ali"), "1234").await;
    let owner = sign_in(id_of("Owner"), "owner1234").await;

    for uri in
        ["/api/dashboard?from=2026-10-01&to=2026-10-10", "/api/inventory", "/api/suppliers", "/api/khata", "/api/users"]
    {
        let (status, body) = call(&app, "GET", uri, Some(&ali), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {body}");
    }

    let (status, d) = call(&app, "GET", "/api/dashboard?from=2026-01-01&to=2030-12-31", Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK, "{d}");
    assert_eq!(d["figures"]["expenses"], 340_000);
    let (status, inv) = call(&app, "GET", "/api/inventory?tab=low", Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK, "{inv}");
    assert!(inv["rows"].as_array().unwrap().iter().any(|r| r["name"] == "Brufen Syrup 90ml"));

    let (status, e) = call(
        &app,
        "POST",
        "/api/expenses",
        Some(&owner),
        Some(json!({ "category": "Tea & food", "amount": 30000, "date": "2026-01-01", "paidFrom": "drawer", "tillCode": "T09" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["reason"], "drawer.no_shift");

    let (status, code) = call(&app, "POST", "/api/recovery-code", Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &app,
        "POST",
        "/api/recover",
        None,
        Some(json!({ "userId": id_of("Owner"), "code": code["code"], "newPassword": "brand-new-pass" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["token"].is_string());
}
