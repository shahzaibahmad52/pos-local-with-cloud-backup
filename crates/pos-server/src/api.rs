//! HTTP API the tills talk to. The main PC serves it on the shop network;
//! every request except setup and login needs a session token.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard},
};

use axum::{
    extract::{ConnectInfo, FromRequestParts, Path, Query, State},
    http::{header, request::Parts, HeaderValue, Method},
    routing::{get, patch, post},
    Json, Router,
};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::{
    auth::{self, Auth, User},
    error::{AppError, AppResult},
    products, returns, rules, sales, shifts, store,
};

#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    auth: Arc<Auth>,
}

impl AppState {
    pub fn new(conn: Connection) -> Self {
        Self { db: Arc::new(Mutex::new(conn)), auth: Arc::new(Auth::default()) }
    }

    fn db(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.db.lock().map_err(|_| AppError::Internal("database lock poisoned".into()))
    }
}

/// The signed-in user, taken from `Authorization: Bearer <token>`.
pub struct AuthUser {
    pub user: User,
    pub token: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?
            .to_string();
        let user_id = state.auth.session_user_id(&token)?.ok_or(AppError::Unauthorized)?;
        let conn = state.db()?;
        let user = auth::load_user(&conn, &user_id)?.ok_or(AppError::Unauthorized)?;
        Ok(AuthUser { user, token })
    }
}

/// Like [`AuthUser`] but only for the owner / admins.
pub struct AdminUser(pub User);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let AuthUser { user, .. } = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(AppError::Forbidden("Only the owner / admin can do this.".into()));
        }
        Ok(AdminUser(user))
    }
}

/// Only requests from this computer itself. First-run setup uses it so another
/// device on the shop Wi-Fi can't claim an empty store before the owner does.
pub struct LocalOnly;

impl FromRequestParts<AppState> for LocalOnly {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &AppState) -> Result<Self, Self::Rejection> {
        match parts.extensions.get::<ConnectInfo<SocketAddr>>() {
            Some(ConnectInfo(addr)) if !addr.ip().is_loopback() => {
                Err(AppError::Forbidden("Set up the store on the main PC itself.".into()))
            }
            // No socket info means an in-process call (tests).
            _ => Ok(LocalOnly),
        }
    }
}

async fn health(State(s): State<AppState>) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    let set_up = store::is_set_up(&conn)?;
    let store = if set_up { Some(sales::store_header(&conn)?) } else { None };
    Ok(Json(json!({ "ok": true, "setUp": set_up, "store": store, "version": env!("CARGO_PKG_VERSION") })))
}

async fn setup(
    State(s): State<AppState>,
    _local: LocalOnly,
    Json(input): Json<store::SetupInput>,
) -> AppResult<Json<Value>> {
    let mut conn = s.db()?;
    let owner = store::setup(&mut conn, &input)?;
    let token = s.auth.start_session(&owner.id)?;
    Ok(Json(json!({ "token": token, "user": owner })))
}

async fn login_tiles(State(s): State<AppState>) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    Ok(Json(json!(store::login_tiles(&conn)?)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginInput {
    user_id: String,
    secret: String,
}

async fn login(State(s): State<AppState>, Json(input): Json<LoginInput>) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    let user = s.auth.check_secret(&conn, &input.user_id, &input.secret)?;
    let token = s.auth.start_session(&user.id)?;
    Ok(Json(json!({ "token": token, "user": user })))
}

async fn logout(State(s): State<AppState>, me: AuthUser) -> AppResult<Json<Value>> {
    s.auth.end_session(&me.token)?;
    Ok(Json(json!({ "ok": true })))
}

async fn me(me: AuthUser) -> Json<User> {
    Json(me.user)
}

#[derive(Deserialize)]
struct LanguageInput {
    language: String,
}

async fn set_language(
    State(s): State<AppState>,
    me: AuthUser,
    Json(input): Json<LanguageInput>,
) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    store::set_language(&conn, &me.user.id, &input.language)?;
    Ok(Json(json!({ "ok": true })))
}

async fn create_user(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<store::NewUser>,
) -> AppResult<Json<User>> {
    let conn = s.db()?;
    Ok(Json(store::create_user(&conn, &input, &admin)?))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

async fn search_products(
    State(s): State<AppState>,
    _me: AuthUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    Ok(Json(json!(products::search(&conn, &q.q, 20)?)))
}

async fn product_by_barcode(
    State(s): State<AppState>,
    _me: AuthUser,
    Path(code): Path<String>,
) -> AppResult<Json<products::BarcodeHit>> {
    let conn = s.db()?;
    products::by_barcode(&conn, &code)?
        .map(Json)
        .ok_or_else(|| AppError::NotFound(format!("No product has barcode {code}.")))
}

async fn create_product(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<products::NewProduct>,
) -> AppResult<Json<products::ProductView>> {
    let mut conn = s.db()?;
    Ok(Json(products::create_product(&mut conn, &input, &admin.id)?))
}

async fn receive_stock(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<products::ReceiveStock>,
) -> AppResult<Json<products::ProductView>> {
    let mut conn = s.db()?;
    Ok(Json(products::receive_stock(&mut conn, &input, &admin.id)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TillQuery {
    till_code: String,
}

async fn current_shift(State(s): State<AppState>, _me: AuthUser, Query(q): Query<TillQuery>) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    Ok(Json(json!(store::open_shift_for(&conn, &q.till_code)?)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenShiftInput {
    till_code: String,
    opening_cash: i64,
}

async fn open_shift(
    State(s): State<AppState>,
    me: AuthUser,
    Json(input): Json<OpenShiftInput>,
) -> AppResult<Json<store::Shift>> {
    let conn = s.db()?;
    Ok(Json(store::open_shift(&conn, &me.user, &input.till_code, input.opening_cash)?))
}

async fn create_sale(
    State(s): State<AppState>,
    me: AuthUser,
    Json(input): Json<sales::SaleInput>,
) -> AppResult<Json<sales::Receipt>> {
    let mut conn = s.db()?;
    Ok(Json(sales::create_sale(&mut conn, &s.auth, &me.user, &input)?))
}

async fn get_receipt(
    State(s): State<AppState>,
    _me: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<sales::Receipt>> {
    let conn = s.db()?;
    sales::receipt(&conn, &id)?.map(Json).ok_or_else(|| AppError::NotFound("Receipt not found.".into()))
}

async fn list_customers(
    State(s): State<AppState>,
    _me: AuthUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    Ok(Json(json!(store::customers(&conn, &q.q)?)))
}

async fn create_customer(
    State(s): State<AppState>,
    me: AuthUser,
    Json(input): Json<store::NewCustomer>,
) -> AppResult<Json<store::CustomerView>> {
    let conn = s.db()?;
    Ok(Json(store::create_customer(&conn, &input, &me.user)?))
}

async fn get_rules(State(s): State<AppState>, _me: AuthUser) -> AppResult<Json<rules::ApprovalRules>> {
    let conn = s.db()?;
    Ok(Json(rules::load(&conn)?))
}

async fn put_rules(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<rules::ApprovalRules>,
) -> AppResult<Json<rules::ApprovalRules>> {
    let conn = s.db()?;
    Ok(Json(rules::save(&conn, &input, &admin)?))
}

async fn record_action(
    State(s): State<AppState>,
    me: AuthUser,
    Json(input): Json<rules::ActionInput>,
) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    Ok(Json(rules::record_action(&conn, &s.auth, &me.user, &input)?))
}

#[derive(Deserialize)]
struct ActivityQuery {
    date: String,
    #[serde(default)]
    filter: String,
}

async fn activity(
    State(s): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<ActivityQuery>,
) -> AppResult<Json<Vec<rules::ActivityEntry>>> {
    let conn = s.db()?;
    Ok(Json(rules::activity(&conn, &q.date, &q.filter)?))
}

async fn sale_for_return(
    State(s): State<AppState>,
    _me: AuthUser,
    Path(receipt_no): Path<String>,
) -> AppResult<Json<returns::SaleForReturn>> {
    let conn = s.db()?;
    Ok(Json(returns::sale_for_return(&conn, &receipt_no)?))
}

async fn create_return(
    State(s): State<AppState>,
    me: AuthUser,
    Json(input): Json<returns::ReturnInput>,
) -> AppResult<Json<returns::ReturnSlip>> {
    let mut conn = s.db()?;
    Ok(Json(returns::create_return(&mut conn, &s.auth, &me.user, &input)?))
}

async fn return_slip(
    State(s): State<AppState>,
    _me: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<returns::ReturnSlip>> {
    let conn = s.db()?;
    returns::return_slip(&conn, &id)?.map(Json).ok_or_else(|| AppError::NotFound("Return not found.".into()))
}

async fn shift_summary(
    State(s): State<AppState>,
    me: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<shifts::ShiftSummary>> {
    let conn = s.db()?;
    Ok(Json(shifts::summary(&conn, &id, me.user.is_admin())?))
}

async fn shift_count(
    State(s): State<AppState>,
    me: AuthUser,
    Path(id): Path<String>,
    Json(input): Json<shifts::CountInput>,
) -> AppResult<Json<shifts::ShiftSummary>> {
    let conn = s.db()?;
    Ok(Json(shifts::count(&conn, &me.user, &id, &input)?))
}

async fn shift_close(
    State(s): State<AppState>,
    me: AuthUser,
    Path(id): Path<String>,
    Json(input): Json<shifts::CloseInput>,
) -> AppResult<Json<shifts::ShiftSummary>> {
    let conn = s.db()?;
    Ok(Json(shifts::close(&conn, &me.user, &id, &input)?))
}

async fn cash_move(
    State(s): State<AppState>,
    me: AuthUser,
    Json(input): Json<shifts::CashMoveInput>,
) -> AppResult<Json<shifts::ShiftSummary>> {
    let conn = s.db()?;
    Ok(Json(shifts::cash_move(&conn, &s.auth, &me.user, &input)?))
}

/// Origins allowed to call the API from a browser: the desktop app's own
/// webview and the local development server. Tills on other PCs use the
/// same desktop app, so their origin is the app's too.
fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            let o = origin.as_bytes();
            o == b"tauri://localhost"
                || o == b"http://tauri.localhost"
                || o == b"https://tauri.localhost"
                || o.starts_with(b"http://localhost:")
                || o.starts_with(b"http://127.0.0.1:")
        }))
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::PUT])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/setup", post(setup))
        .route("/api/login-tiles", get(login_tiles))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/language", patch(set_language))
        .route("/api/users", post(create_user))
        .route("/api/products", post(create_product))
        .route("/api/products/search", get(search_products))
        .route("/api/products/barcode/{code}", get(product_by_barcode))
        .route("/api/stock/receive", post(receive_stock))
        .route("/api/shifts/current", get(current_shift))
        .route("/api/shifts/open", post(open_shift))
        .route("/api/sales", post(create_sale))
        .route("/api/sales/{id}/receipt", get(get_receipt))
        .route("/api/customers", get(list_customers).post(create_customer))
        .route("/api/rules", get(get_rules).put(put_rules))
        .route("/api/actions", post(record_action))
        .route("/api/activity", get(activity))
        .route("/api/returns", post(create_return))
        .route("/api/returns/sale/{receipt_no}", get(sale_for_return))
        .route("/api/returns/{id}/slip", get(return_slip))
        .route("/api/shifts/{id}/summary", get(shift_summary))
        .route("/api/shifts/{id}/count", post(shift_count))
        .route("/api/shifts/{id}/close", post(shift_close))
        .route("/api/cash-moves", post(cash_move))
        .layer(cors())
        .with_state(state)
}
