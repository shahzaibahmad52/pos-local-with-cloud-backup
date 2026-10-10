//! HTTP API the tills talk to. The main PC serves it on the shop network;
//! every request except setup and login needs a session token.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard},
};

use axum::{
    extract::{ConnectInfo, FromRequestParts, Path, Query, State},
    http::{header, request::Parts, HeaderValue, Method},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::{
    auth::{self, Auth, User},
    dashboard,
    error::{AppError, AppResult},
    expenses, import, inventory, khata, products, purchases, returns, rules, sales, settings, shifts, store,
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

async fn dashboard(
    State(s): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<dashboard::Period>,
) -> AppResult<Json<dashboard::Dashboard>> {
    let conn = s.db()?;
    Ok(Json(dashboard::dashboard(&conn, &q)?))
}

async fn inventory_list(
    State(s): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<inventory::InventoryQuery>,
) -> AppResult<Json<inventory::InventoryPage>> {
    let conn = s.db()?;
    Ok(Json(inventory::list(&conn, &q)?))
}

async fn inventory_detail(
    State(s): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<inventory::ProductDetail>> {
    let conn = s.db()?;
    Ok(Json(inventory::detail(&conn, &id)?))
}

async fn update_product(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
    Json(input): Json<inventory::ProductUpdate>,
) -> AppResult<Json<inventory::ProductDetail>> {
    let mut conn = s.db()?;
    Ok(Json(inventory::update(&mut conn, &admin, &id, &input)?))
}

async fn adjust_stock(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<inventory::AdjustInput>,
) -> AppResult<Json<inventory::ProductDetail>> {
    let mut conn = s.db()?;
    Ok(Json(inventory::adjust(&mut conn, &admin, &input)?))
}

#[derive(Deserialize)]
struct ImportInput {
    rows: Vec<import::ImportRow>,
    #[serde(default)]
    commit: bool,
}

async fn import_products(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<ImportInput>,
) -> AppResult<Json<import::ImportReport>> {
    let mut conn = s.db()?;
    Ok(Json(import::run(&mut conn, &admin, &input.rows, input.commit)?))
}

async fn list_suppliers(State(s): State<AppState>, _admin: AdminUser) -> AppResult<Json<Vec<purchases::SupplierView>>> {
    let conn = s.db()?;
    Ok(Json(purchases::suppliers(&conn)?))
}

async fn create_supplier(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<purchases::SupplierInput>,
) -> AppResult<Json<purchases::SupplierView>> {
    let conn = s.db()?;
    Ok(Json(purchases::create_supplier(&conn, &input, &admin)?))
}

async fn update_supplier(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
    Json(input): Json<purchases::SupplierInput>,
) -> AppResult<Json<purchases::SupplierView>> {
    let conn = s.db()?;
    Ok(Json(purchases::update_supplier(&conn, &id, &input, &admin)?))
}

async fn supplier_ledger(
    State(s): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<Vec<purchases::LedgerLine>>> {
    let conn = s.db()?;
    Ok(Json(purchases::supplier_ledger(&conn, &id)?))
}

async fn pay_supplier(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
    Json(input): Json<purchases::SupplierPaymentInput>,
) -> AppResult<Json<purchases::SupplierView>> {
    let mut conn = s.db()?;
    Ok(Json(purchases::pay_supplier(&mut conn, &admin, &id, &input)?))
}

async fn supplier_opening(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
    Json(input): Json<purchases::OpeningInput>,
) -> AppResult<Json<purchases::SupplierView>> {
    let conn = s.db()?;
    Ok(Json(purchases::add_opening(&conn, &admin, &id, &input)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PurchaseQuery {
    supplier_id: Option<String>,
}

async fn list_purchases(
    State(s): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<PurchaseQuery>,
) -> AppResult<Json<Vec<purchases::PurchaseView>>> {
    let conn = s.db()?;
    Ok(Json(purchases::purchases(&conn, q.supplier_id.as_deref().filter(|v| !v.is_empty()), 100)?))
}

async fn receive_purchase(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<purchases::PurchaseInput>,
) -> AppResult<Json<purchases::PurchaseView>> {
    let mut conn = s.db()?;
    Ok(Json(purchases::receive(&mut conn, &admin, &input)?))
}

async fn get_purchase(
    State(s): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<purchases::PurchaseView>> {
    let conn = s.db()?;
    purchases::purchase(&conn, &id)?.map(Json).ok_or_else(|| AppError::NotFound("Purchase not found.".into()))
}

async fn last_buy(State(s): State<AppState>, _admin: AdminUser, Path(id): Path<String>) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    Ok(Json(json!(purchases::last_buy(&conn, &id)?)))
}

#[derive(Deserialize)]
struct MonthQuery {
    month: String,
}

async fn list_expenses(
    State(s): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<MonthQuery>,
) -> AppResult<Json<expenses::MonthExpenses>> {
    let conn = s.db()?;
    Ok(Json(expenses::month(&conn, &q.month)?))
}

async fn add_expense(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<expenses::ExpenseInput>,
) -> AppResult<Json<expenses::ExpenseView>> {
    let mut conn = s.db()?;
    Ok(Json(expenses::add(&mut conn, &admin, &input)?))
}

async fn delete_expense(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let mut conn = s.db()?;
    expenses::delete(&mut conn, &admin, &id)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct KhataQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    tab: String,
}

async fn khata_list(
    State(s): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<KhataQuery>,
) -> AppResult<Json<khata::AccountList>> {
    let conn = s.db()?;
    Ok(Json(khata::list(&conn, &q.q, &q.tab)?))
}

async fn khata_statement(
    State(s): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Json<khata::Statement>> {
    let conn = s.db()?;
    Ok(Json(khata::statement(&conn, &id)?))
}

async fn khata_payment(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
    Json(input): Json<khata::PaymentInput>,
) -> AppResult<Json<khata::PaymentSlip>> {
    let mut conn = s.db()?;
    Ok(Json(khata::receive_payment(&mut conn, &admin, &id, &input)?))
}

async fn khata_opening(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
    Json(input): Json<khata::OpeningInput>,
) -> AppResult<Json<khata::Account>> {
    let conn = s.db()?;
    Ok(Json(khata::add_opening(&conn, &admin, &id, &input)?))
}

async fn update_customer(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
    Json(input): Json<khata::CustomerUpdate>,
) -> AppResult<Json<khata::Account>> {
    let conn = s.db()?;
    Ok(Json(khata::update(&conn, &admin, &id, &input)?))
}

async fn open_tills(State(s): State<AppState>, _admin: AdminUser) -> AppResult<Json<Vec<shifts::OpenTill>>> {
    let conn = s.db()?;
    Ok(Json(shifts::open_tills(&conn)?))
}

async fn get_settings(State(s): State<AppState>, _admin: AdminUser) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    Ok(Json(json!({
        "store": sales::store_header(&conn)?,
        "hasRecoveryCode": settings::has_recovery_code(&conn)?,
        "version": env!("CARGO_PKG_VERSION"),
    })))
}

async fn update_store(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(input): Json<settings::StoreInput>,
) -> AppResult<Json<sales::StoreHeader>> {
    let mut conn = s.db()?;
    Ok(Json(settings::update_store(&mut conn, &admin, &input)?))
}

async fn list_users(State(s): State<AppState>, _admin: AdminUser) -> AppResult<Json<Vec<settings::UserRow>>> {
    let conn = s.db()?;
    Ok(Json(settings::users(&conn)?))
}

async fn update_user(
    State(s): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<String>,
    Json(input): Json<settings::UserUpdate>,
) -> AppResult<Json<settings::UserRow>> {
    let conn = s.db()?;
    Ok(Json(settings::update_user(&conn, &admin, &id, &input)?))
}

async fn new_recovery_code(State(s): State<AppState>, AdminUser(admin): AdminUser) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    Ok(Json(json!({ "code": settings::new_recovery_code(&conn, &admin)? })))
}

/// Only on the main PC itself: someone on the shop Wi-Fi shouldn't be able to
/// try recovery codes.
async fn recover(
    State(s): State<AppState>,
    _local: LocalOnly,
    Json(input): Json<settings::RecoverInput>,
) -> AppResult<Json<Value>> {
    let conn = s.db()?;
    let user = settings::recover(&conn, &s.auth, &input)?;
    let token = s.auth.start_session(&user.id)?;
    Ok(Json(json!({ "token": token, "user": user })))
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
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::PUT, Method::DELETE])
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
        .route("/api/users", get(list_users).post(create_user))
        .route("/api/users/{id}", put(update_user))
        .route("/api/settings", get(get_settings))
        .route("/api/store", put(update_store))
        .route("/api/recovery-code", post(new_recovery_code))
        .route("/api/recover", post(recover))
        .route("/api/dashboard", get(dashboard))
        .route("/api/inventory", get(inventory_list))
        .route("/api/inventory/{id}", get(inventory_detail))
        .route("/api/products", post(create_product))
        .route("/api/products/import", post(import_products))
        .route("/api/products/{id}", put(update_product))
        .route("/api/products/search", get(search_products))
        .route("/api/products/barcode/{code}", get(product_by_barcode))
        .route("/api/stock/receive", post(receive_stock))
        .route("/api/stock/adjust", post(adjust_stock))
        .route("/api/suppliers", get(list_suppliers).post(create_supplier))
        .route("/api/suppliers/{id}", put(update_supplier))
        .route("/api/suppliers/{id}/ledger", get(supplier_ledger))
        .route("/api/suppliers/{id}/payments", post(pay_supplier))
        .route("/api/suppliers/{id}/opening", post(supplier_opening))
        .route("/api/purchases", get(list_purchases).post(receive_purchase))
        .route("/api/purchases/{id}", get(get_purchase))
        .route("/api/purchases/last-buy/{product_id}", get(last_buy))
        .route("/api/expenses", get(list_expenses).post(add_expense))
        .route("/api/expenses/{id}", delete(delete_expense))
        .route("/api/khata", get(khata_list))
        .route("/api/khata/{id}", get(khata_statement))
        .route("/api/khata/{id}/payments", post(khata_payment))
        .route("/api/khata/{id}/opening", post(khata_opening))
        .route("/api/customers/{id}", put(update_customer))
        .route("/api/tills/open", get(open_tills))
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
