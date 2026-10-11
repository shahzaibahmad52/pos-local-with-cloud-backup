# Bizflow POS — Technical Documentation

Bizflow POS is an offline-first point-of-sale and back-office system for medical stores and general stores in Pakistan. Each store runs on its own main PC with a local SQLite database, so the shop keeps selling with no internet. Extra counters connect over the shop network. When cloud backup is on, every change is copied to a per-store Turso database, and the owner can compare all branches from one screen.

| | |
|---|---|
| **End-user guide** | [docs/Bizflow-POS-User-Manual.pdf](docs/Bizflow-POS-User-Manual.pdf) (62 pages, with screenshots; source in [`docs/manual/`](docs/manual)) |
| **UI design** | https://claude.ai/artifact/DdLH1Rp4aDxSGTM2m2EKUz |
| **Sample product sheet** | [samples/Bizflow-medicines-pakistan.xlsx](samples/Bizflow-medicines-pakistan.xlsx) (108 medicines in import format) |
| **Version** | 0.1.0 · app identifier `pk.medstore.pos` |

---

## Contents

1. [System overview](#1-system-overview)
2. [Architecture](#2-architecture)
3. [Repository layout](#3-repository-layout)
4. [Technology stack](#4-technology-stack)
5. [Developer setup](#5-developer-setup)
6. [Configuration reference](#6-configuration-reference)
7. [Domain model and business rules](#7-domain-model-and-business-rules)
8. [Database](#8-database)
9. [HTTP API](#9-http-api)
10. [Authentication and security](#10-authentication-and-security)
11. [Offline selling on extra tills](#11-offline-selling-on-extra-tills)
12. [Cloud backup, restore and multi-store](#12-cloud-backup-restore-and-multi-store)
13. [FBR invoice reporting](#13-fbr-invoice-reporting)
14. [Printing](#14-printing)
15. [Frontend](#15-frontend)
16. [Desktop shell (Tauri)](#16-desktop-shell-tauri)
17. [Testing and CI](#17-testing-and-ci)
18. [Building and releasing](#18-building-and-releasing)
19. [Deployment and operations](#19-deployment-and-operations)
20. [Extending the system](#20-extending-the-system)
21. [Known limitations](#21-known-limitations)
22. [Feature history](#22-feature-history)

---

## 1. System overview

### What it does

| Area | Capabilities |
|---|---|
| **Till** | Barcode / name / formula search; sell by box, strip, tablet or by weight (Rs amount or grams); bill discounts; cash (with change), card, JazzCash, Easypaisa, khata; manager-PIN approvals; 80 mm / 58 mm receipts with a Code128 barcode; English / Urdu (RTL). |
| **Daily control** | Shifts with opening cash; blind closing count by note denomination; Z-report; cash in/out; returns against a receipt with condition and reason; configurable approval rules; append-only activity log. |
| **Inventory** | Products with up to N pack levels; per-batch stock, cost, expiry and printed price; FEFO selling; stock corrections; stock counts by rack; branch transfers; Excel import. |
| **Purchasing** | GRNs with batch, expiry, bonus packs, trade price, line discount, MRP and freight; supplier ledgers and payables with due dates. |
| **Money** | Expenses; customer khata with credit limits, ageing, WhatsApp reminders and statements; dashboard; 19 financial and stock reports with print/PDF and Excel export. |
| **Resilience** | Fully local operation; extra tills sell offline from a cached catalogue; daily local backup files; continuous cloud backup to Turso; full restore onto a new PC. |
| **Multi-store** | One database per store; head-office view of every store's figures; stock transfers between branches. |
| **Compliance** | Optional FBR (PRAL) invoice reporting with invoice number and QR code on receipts. |

### Design principles

- **Local first.** The main PC's database is the source of truth. The internet is never on the path of a sale.
- **Exact money.** All amounts are integer **paisa** (`i64`). Release builds keep `overflow-checks = true`, so an overflow is an error and never a silent wrong total.
- **Stock in the smallest unit.** Tablets, pieces or grams. Packs are views over that unit.
- **The server decides.** While online, the till never sets prices. The server recomputes every line. The till's copy of the maths (`src/domain/pricing.ts`) only shows running totals, and is tested against the same cases as the Rust version.
- **Every sensitive action is recorded** in an append-only audit log that the database itself refuses to update or delete.
- **Retries are safe.** Sales, returns, purchases and transfers carry a client reference, so a resend after a dropped connection is recorded once.

---

## 2. Architecture

```
 Store (one shop)                                                   Cloud (optional)
 ┌───────────────────────────────────────────────────────────┐
 │ Main PC                                                   │
 │  ┌─────────────── Bizflow POS desktop app ──────────────┐ │
 │  │ WebView (React UI) ──HTTP──► pos-server (axum) :7420 │ │   sync_queue ─► Turso (libSQL)
 │  │        ▲                         │                   │ │   every 30 s    one DB per store
 │  │        │ Tauri IPC               ▼                   │ │                    ▲
 │  │  print_raster ─► pos-printer   SQLite (WAL)          │ │                    │ read-only
 │  │                  ESC/POS ─► thermal printer + drawer │ │                    │ tokens
 │  └──────────────────────────────────────────────────────┘ │            Head-office view,
 │        ▲  shop LAN / Wi-Fi (HTTP, Bearer token)            │            incoming transfers
 │ Extra till(s)                                             │
 │  Bizflow POS desktop app (UI only, offline catalogue)     │   FBR / PRAL (optional)
 └───────────────────────────────────────────────────────────┘   fbr_queue ─► PostData
```

### Components

| Component | Path | Role |
|---|---|---|
| **pos-server** | `crates/pos-server` | Rust library and binary. Owns the SQLite database, all business rules and the HTTP API. Runs background work: cloud sync, the FBR queue and daily backups. |
| **pos-printer** | `crates/pos-printer` | Turns a 1-bit raster image into ESC/POS bytes (`GS v 0`, cut, drawer pulse) and sends raw bytes to a Windows printer through the spooler. |
| **Desktop shell** | `src-tauri` (crate `medstore-pos`) | Tauri 2 app. On a main PC it starts `pos-server` in-process on `0.0.0.0:7420`. On an extra till it points the UI at the main PC. Exposes printing, file save, WhatsApp and restart commands. |
| **UI** | `src` | React 19 + TypeScript, built by Vite. One app for the till and the backoffice. |

### Request flow of a sale

1. The till builds the cart locally and shows totals using `src/domain/pricing.ts` and `cart.ts`.
2. **Pay** sends `POST /api/sales` with lines (unit id + qty, or grams / amount for weight items), discount, payments, an optional customer and approval, and a `clientRef` UUID.
3. The server, in a single transaction:
   - Re-prices every line, using the batch's printed price when there is one.
   - Allocates stock **FEFO** across batches, skipping expired ones.
   - Checks the approval rules and the khata credit limit, and requires an open shift.
   - Numbers the receipt per till.
   - Writes the sale, its lines, the batch allocations, payments, stock moves and khata entry. It audits and queues FBR when enabled.
4. The receipt JSON comes back. The UI renders it on screen and, on the desktop, rasterises it and prints it directly.

---

## 3. Repository layout

```
crates/
  pos-server/
    migrations/           001–008 SQL schema migrations (embedded with include_str!)
    src/
      lib.rs              run(): open DB, bind listener, spawn background loop
      main.rs             CLI: --db --host --port --demo
      api.rs              Router, extractors (AuthUser / AdminUser / LocalOnly), CORS, handlers
      auth.rs             Argon2 hashing, sessions, lockouts, manager approvals
      error.rs            AppError → HTTP status + {error, message, reason, params}
      db.rs               Pragmas, migrations, ids, time, settings, audit helper
      pricing.rs          Integer money maths (rounding, weight, included tax, %)
      sales.rs            Sale creation, FEFO allocation, receipts, offline-sale validation
      returns.rs          Returns/refunds against a sale
      shifts.rs           Cash moves, blind count, close, Z-report data
      rules.rs            Approval rules and activity log
      store.rs            First-run setup, users, shifts, customers
      products.rs         Search, barcode lookup, catalogue + price lists, create/receive
      inventory.rs        Inventory list/detail, product edit, stock correction
      import.rs           Excel import validation and commit
      purchases.rs        Suppliers, GRNs, supplier payments
      ledger.rs           Running balances, oldest-first settlement, ageing
      khata.rs            Customer accounts, payments, opening balances
      expenses.rs         Expenses (optionally out of a till drawer)
      dashboard.rs        Owner dashboard figures
      reports.rs          19 reports, one entry point
      counts.rs           Stock counts
      transfers.rs        Branch transfers (transfer notes)
      fbr.rs              FBR/PRAL invoice queue and sender
      settings.rs         Store details, receipt text, users, recovery code
      seed.rs             Demo store (only with --demo)
      cloud/
        hrana.rs          Minimal Turso "Hrana over HTTP" client
        sync.rs           Change tracking triggers, upload, enable/disable, restore
        backup.rs         Daily VACUUM INTO backup files
        hq.rs             Head-office figures and incoming transfers from other stores
    tests/                Integration tests (sales, returns/shifts, backoffice, reports,
                          stock ops, cloud, FBR, API, QA regressions)
  pos-printer/
    src/escpos.rs         ESC/POS encoding
    src/windows_spooler.rs  Raw printing via winspool (Windows only)
src/
  main.tsx, App.tsx       Boot: runtime → health → setup / login / till / backoffice
  runtime.ts, platform.ts Desktop vs browser detection, device config, saveFile()
  api/                    client.ts (typed fetch wrapper), types.ts (API types)
  domain/                 pricing, cart, money parsing, offline queue, barcode, dates,
                          stock display, purchase maths, import sheet, WhatsApp links
  print/                  doc.ts (document model), DocView.tsx (screen), raster.ts
                          (canvas → 1-bit), code128.ts, qr.ts, printer.ts
  screens/                DeviceSetup, StoreSetup, Login, OpenShift, Till, Returns,
                          ShiftClose, Activity
  backoffice/             Dashboard, Reports, Inventory, ProductForm, ImportProducts,
                          StockCount, Transfers, Purchases, Suppliers, Expenses,
                          Customers, HeadOffice, Settings, Cloud, Fbr
  components/             PinPad, ApprovalDialog, CashMoveDialog, PrinterSetup, common
  i18n/                   en/ur strings per area, error translations, coverage test
src-tauri/                Desktop shell, tauri.conf.json, icons, capabilities
docs/                     User manual PDF and its HTML/CSS source + build script
samples/                  Example import workbook
.github/workflows/        ci.yml, windows-installer.yml
```

---

## 4. Technology stack

| Layer | Technology |
|---|---|
| Server | Rust 2021, **axum 0.8**, tokio, **rusqlite 0.32** (bundled SQLite, backup API), argon2, uuid v7, chrono, reqwest (rustls), tower-http CORS |
| Database | SQLite in WAL mode; Turso / libSQL in the cloud |
| UI | **React 19**, TypeScript 5.9, Vite 7, Vitest 5 |
| UI libraries | `read-excel-file` / `write-excel-file` (import/export), `qrcode-generator` (FBR QR), self-hosted IBM Plex Sans/Mono and Noto Nastaliq Urdu fonts |
| Desktop | **Tauri 2** (WebView2 on Windows), NSIS installer |
| Printing | ESC/POS raster (`GS v 0`), Windows spooler raw printing via the `windows` crate |
| CI | GitHub Actions (Ubuntu + Windows runners) |

---

## 5. Developer setup

### Prerequisites

- **Node 22** and npm
- **Rust stable** (`rustup`)
- For the desktop app on Windows: Visual Studio Build Tools with *Desktop development with C++*. WebView2 ships with Windows 10/11.
- For the desktop app on Linux (CI or dev): `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev`

### Run in a browser (any OS)

```bash
npm install
npm run server:demo      # terminal 1: pos-server with demo data on 127.0.0.1:7420 (.dev/store.sqlite)
npm run dev              # terminal 2: Vite on http://localhost:5173 (proxies /api → :7420)
```

In the browser, printing falls back to the print dialog, and downloads use a normal browser download.

### Run the desktop app

```bash
npm install
npx tauri dev            # starts Vite and the Tauri window
```

On first start, choose **This is the main PC**. Set `MEDSTORE_DEMO=1` beforehand to load the demo store into an empty database.

### Demo data

`--demo` (or `MEDSTORE_DEMO=1`) seeds a store with sample products and batches, including a partly expired Flagyl batch. It also adds two suppliers, one purchase, a Rs 3,400 fuel expense and an old khata balance for Haji Rasheed.

| User | Role | Login |
|---|---|---|
| Owner | Admin | password `owner1234` |
| Ali | Cashier | PIN `1234` |
| Sana | Cashier, can approve, Urdu | PIN `2345` |
| Usman | Cashier | PIN `3456` |

Things to try:
- Scan `8964000123463` (Panadol strip).
- Sell "Rs 10" of sugar.
- Give a 10% discount, which needs Sana's PIN.
- Sell 15 strips of Flagyl, which is refused because part of its stock is expired.

### Running the server on its own

```bash
cargo run -p pos-server -- --db ./store.sqlite --host 0.0.0.0 --port 7420 [--demo]
```

---

## 6. Configuration reference

### `pos-server` command line

| Flag | Default | Meaning |
|---|---|---|
| `--db PATH` | `./store.sqlite` | Database file. Its folder is created if missing. Backups go to `<folder>/backups`. |
| `--host HOST` | `0.0.0.0` | Listen address (all interfaces = the shop network). |
| `--port PORT` | `7420` | Listen port. |
| `--demo` | off | Seed the demo store into an **empty** database. |

### Environment variables

| Variable | Used by | Meaning |
|---|---|---|
| `MEDSTORE_DEMO=1` | desktop shell | Start the embedded server with `--demo`. |
| `PLAYWRIGHT_PATH` | `docs/manual/build.cjs` | Where to load Playwright from if it isn't installed in the project. |

### Device configuration (`device.json`)

Stored by the desktop shell in the app data folder (`%APPDATA%\pk.medstore.pos\device.json` on Windows). It is written by the first-run screen and the printer dialog.

```json
{
  "mode": "main",                 // "main" | "till"
  "mainHost": "192.168.1.10",     // extra tills only
  "port": 7420,                   // optional; keep 7420 (see CSP below)
  "tillCode": "T01",              // 1–6 letters/digits, printed on receipts
  "printer": { "name": "EPSON TM-T20III Receipt", "widthDots": 576,
               "autoPrint": true, "openDrawerOnCash": true, "cut": true }
}
```

The server URL is `http://127.0.0.1:<port>` on a main PC and `http://<mainHost>:<port>` on a till.

> The Tauri CSP allows API calls to `http://*:7420` only. To use a different port, change `connect-src` in `src-tauri/tauri.conf.json` too.

### Store settings (database `settings` table)

| Key | Default | Meaning |
|---|---|---|
| `receipt_footer`, `receipt_footer_ur` | — | Last line of the receipt (English / Urdu). |
| `receipt_policy`, `receipt_policy_ur` | built-in returns text | Returns policy printed above the footer. An empty string prints none. |
| `receipt_language` | `en` | Default receipt language (`en`, `ur`, `both`). A customer's own choice overrides it. |
| `cashier_max_discount_bp` | `500` | Discount (basis points) a cashier may give without approval. |
| `refund_approval_above` | `50000` | Refund (paisa) above which approval is needed. Cumulative per sale. |
| `rule_remove_item` / `rule_cancel_bill` / `rule_cash_out` / `rule_no_sale_drawer` | `1` / `1` / `0` / `1` | Actions that need a manager PIN. |
| `weight_round_to_paisa` | `100` | Rounding for weight-item prices (100 = nearest rupee). |
| `cloud_enabled`, `cloud_url`, `cloud_token`, `cloud_last_sync` | — | Cloud backup. **Never synced.** |
| `hq_stores` | — | Other stores' cloud URLs and read-only tokens. **Never synced.** |
| `fbr_enabled`, `fbr_mode`, `fbr_pos_id`, `fbr_url`, `fbr_default_pct` | off / sandbox / — / — / `30049099` | FBR reporting. |
| `fbr_token` | — | FBR API token. **Never synced.** |
| `recovery_code_hash` | — | Argon2 hash of the owner recovery code. |

### Browser storage on each till (`localStorage`)

| Key | Contents |
|---|---|
| `medstore.catalogue` | Last downloaded catalogue and price list id (offline selling). |
| `medstore.offline.queue` | Offline sales waiting to be sent. |
| `medstore.offline.refused` | Offline sales the main PC refused, kept for the owner. |
| `medstore.offline.no.<till>` | Last X receipt number used by this till. |
| `medstore.printer`, `medstore.tillCode` | Browser-mode printer and till code. |

The session token is held **in memory only**, so closing the window signs the user out.

### Data locations (Windows)

| What | Where |
|---|---|
| Store database | `%APPDATA%\pk.medstore.pos\store.sqlite` (+ `-wal`, `-shm`) |
| Daily backups | `%APPDATA%\pk.medstore.pos\backups\store-YYYY-MM-DD-HHMMSS.sqlite` (last 14) |
| Device config | `%APPDATA%\pk.medstore.pos\device.json` |
| Saved downloads | The user's *Downloads* folder; existing files are never overwritten (`name (2).xlsx`). |

---

## 7. Domain model and business rules

### Units, packs and prices

- `products.base_unit` is the smallest unit. All stock (`batches.qty_on_hand`, `stock_moves.qty_delta`) is in base units. Weight items use **grams**.
- `product_units` define pack levels, e.g. Tab ×1, Strip ×10, Box ×200. Each has a default sale price and a `sellable` flag. Urdu names are optional.
- `barcodes` map a code to a product **and** a pack, so a box barcode adds a box.
- Weight items (`sell_mode = 'weight'`) have `price_per_kg`. The till sells them by grams or by rupee amount:
  - `amount = round(grams × price_per_kg / 1000)`, rounded to `weight_round_to_paisa`.
  - `grams = round(amount × 1000 / price_per_kg)`.
- Prices are **tax-inclusive**. `included_tax = round(amount × rate / (10000 + rate))` per line, with the rate in basis points.
- Rounding is **half away from zero** everywhere (`pricing::round_div`, mirrored in TypeScript).
- Input bounds: amounts ≤ `MAX_AMOUNT` (Rs 10 crore in paisa), quantities ≤ `MAX_QTY` (10,000,000).

### Batches and FEFO

- Each receipt of stock creates or extends a **batch**: `batch_no`, `expiry` (`YYYY-MM`, usable until the end of that month), `cost_per_base` and `qty_on_hand`.
- `batch_unit_prices` stores the MRP printed on that batch's packs. It overrides the product's default price for that batch. Changing a pack's size clears batch prices.
- Sales allocate **first-expiry-first-out** across batches. Expired batches are never sold. A sale line can span batches (`sale_line_batches` records each part with its cost), and the receipt shows the batch sold.
- Cost of goods comes from the batches actually consumed, so profit reports are exact.

### Receipts and document numbers

| Document | Format | Example |
|---|---|---|
| Sale | `<store>-<till>-<6 digits>` | `B1-T01-000013` |
| Offline sale | `<store>-<till>-X<6 digits>` | `B1-T02-X000001` |
| Return | `<store>-<till>-R<5 digits>` | `B1-T01-R00003` |
| Purchase (GRN) | `P-<5 digits>` | `P-00001` |
| Stock count | `SC-<4 digits>` | `SC-0001` |
| Transfer | `<store>-TR-<4 digits>` | `B1-TR-0001` |

Sale numbers come from `receipt_counters`, per till. `receipt_aliases` keeps any offline number that was printed for a sale which the server had in fact saved, so that number still finds the sale for a return.

### Shifts and cash

- At most **one open shift per till**, enforced by a partial unique index. Sales, returns and cash moves need an open shift (except offline sales, which already happened).
- Cash is recorded as **what stayed in the drawer**: tendered minus change, with the change taken once per bill.
- Expected cash = opening + cash sales + cash in − cash out − cash refunds − cash expenses / supplier payments made from the drawer.
- Closing count:
  - Cashiers count **blind**, by denomination (5000…10, plus coins); the expected figure is hidden until they submit.
  - One recount is allowed.
  - A difference over Rs 100 needs a note.
  - Closing locks the shift and produces Z-report data.
- A cash refund or cash-out can never exceed the cash in the drawer.

### Approvals

- `ApprovalRules` (section 6) decide which actions need a manager. The approver is the owner/admin or a cashier with `can_approve`. They type their PIN or password into the requesting till.
- The request carries `{ approverId, secret }`. It is checked once, up front, and the approver is stored on the sale/return/move and in the audit log.
- Wrong tries are counted per *(requester, approver)* pair, so a cashier guessing can't lock the manager out of their own sign-in.

### Khata (customer credit)

- `khata_entries.amount > 0` is a charge (bill, old balance); `< 0` is a payment or refund.
- Payments settle the **oldest charges first** (`ledger.rs`), which gives the age of what is still unpaid. Refunds settle their own bill.
- A bill that would exceed `credit_limit` needs approval. `credit_limit = 0` means no khata.
- A customer is **overdue** when their oldest unpaid charge is more than 30 days old.
- A cash payment can go into a till's drawer so the shift's expected cash includes it.

### Returns

- They are found by receipt number, by scanning the receipt barcode, or by an alias.
- Per line: quantity (in the sale line's units), reason and condition:
  - `restock`: back into the **original batch** at its original cost.
  - `damaged` / `expired`: written off, visible in the P&L.
- The refund is the line's share **after the bill discount**. It can never exceed what was paid, and the last return of a sale settles the remainder.
- Refund method:
  - cash, card or wallet;
  - khata credit (customers only);
  - a sale paid by khata is refunded to khata only.
- Fridge (`cold_chain`) products are not returnable. Refunds over the limit need approval; the check is cumulative per sale.

### Purchases (GRN)

- Per line: pack, batch, expiry, qty, **bonus** (free packs), trade price, discount %, optional MRP.
- **Cost per base unit** includes bonus units and a share of freight. Freight is spread by line value, and any leftover paisa goes to the largest line.
- `paid_now` can come from owner cash, bank, wallet or a till drawer. The rest goes to `supplier_entries` with a due date from the payment terms.

### Stock counts and transfers

- **Counts** snapshot each batch's system quantity at the moment it is counted, so sales during the count are kept. Applying changes stock only by the counted difference, with reasons. A recount re-snapshots.
- **Transfers** send whole batches (number, expiry, cost) to another store as a self-contained *transfer note* (`kind: "medstore-transfer"`). The note includes product definitions, so the receiver can create products it has never stocked.
- Receiving is idempotent: `transfers.source_id` is unique. A send carries a `client_ref`. Expired batches can't be sent, and pack definitions must match on both sides.

---

## 8. Database

### Engine settings

`db::open` sets `journal_mode = WAL`, `synchronous = NORMAL`, `foreign_keys = ON`, `busy_timeout = 5000`. One `rusqlite::Connection` sits behind a `Mutex` in `AppState`. A poisoned mutex is recovered rather than taking the server down. Ids are **UUID v7** (time-ordered). Timestamps are local RFC 3339 with offset (`2026-10-11T14:32:05+05:00`).

### Migrations

`crates/pos-server/migrations/NNN_*.sql` files are embedded in the binary in order. `PRAGMA user_version` records how many have run, and each new one runs in its own transaction at start-up. The same list builds the schema in the cloud database (section 12). **Never edit a released migration; add a new one.**

| # | File | Adds |
|---|---|---|
| 001 | `init` | settings, stores, users, products, product_units, barcodes, batches, batch_unit_prices, stock_moves, customers, shifts, receipt_counters, sales, sale_lines, sale_line_batches, payments, khata_entries, audit_log (+ append-only triggers), outbox |
| 002 | `returns_shifts` | returns, return_lines, cash_moves; shift count fields |
| 003 | `backoffice` | suppliers, purchases, purchase_lines, supplier_entries, expenses; customers.active, products.supplier_id |
| 004 | `cloud_sync` | sync_queue |
| 005 | `counts_transfers` | stock_counts, stock_count_lines, transfers, transfer_lines |
| 006 | `fbr` | fbr_queue; sales/returns.fbr_invoice_no |
| 007 | `offline_price_lists` | offline_price_lists |
| 008 | `retry_safety` | transfers.client_ref (unique), receipt_aliases |

### Table reference

| Group | Tables | Notes |
|---|---|---|
| Store & people | `stores`, `users`, `settings` | `users.secret_hash` is Argon2 (PIN for cashiers, password for admins). `role ∈ {admin, cashier}`. |
| Catalogue | `products`, `product_units`, `barcodes` | `kind ∈ {medicine, general}`, `sell_mode ∈ {unit, weight}`, `tax_rate_bp`, `rx_required`, `controlled`, `cold_chain`, `rack`, `reorder_level` (base units), `active`. |
| Stock | `batches`, `batch_unit_prices`, `stock_moves` | `stock_moves.reason`: opening, purchase, sale, return, adjust, count, transfer_in, transfer_out (opening stock from the product form or Excel import). |
| Selling | `shifts`, `receipt_counters`, `sales`, `sale_lines`, `sale_line_batches`, `payments`, `receipt_aliases` | `sales.client_ref` is unique (safe retries). `payments.method ∈ {cash, card, jazzcash, easypaisa, khata}`. |
| Returns & cash | `returns`, `return_lines`, `cash_moves` | `condition ∈ {restock, damaged, expired}`. |
| Customers | `customers`, `khata_entries` | `receipt_language ∈ {en, ur, both}`. |
| Purchasing | `suppliers`, `purchases`, `purchase_lines`, `supplier_entries` | `purchase_lines.cost_per_base` includes bonus and freight. |
| Expenses | `expenses` | `paid_from ∈ {drawer, owner, bank, wallet}`; a drawer expense links to its `cash_move`. |
| Stock ops | `stock_counts`, `stock_count_lines`, `transfers`, `transfer_lines` | |
| Audit | `audit_log` | **Append-only**: `BEFORE UPDATE/DELETE` triggers raise an error. `details` is JSON. |
| Integration | `sync_queue`, `fbr_queue`, `offline_price_lists`, `outbox` | `sync_queue` and `offline_price_lists` are local only (never uploaded). `outbox` is legacy and unused. |

### Reading the data

Money columns are paisa. Divide by 100 for rupees. Expiry is `YYYY-MM`. Business dates (`invoice_date`, `expense_date`) are `YYYY-MM-DD`. The database can be opened with any SQLite tool while the app is closed; while it runs, open it read-only.

---

## 9. HTTP API

Base URL: `http://<main-pc>:7420`. Every body is JSON with **camelCase** fields. The till's typed client is `src/api/client.ts`, and its types are in `src/api/types.ts`.

### Conventions

- **Auth:** `Authorization: Bearer <token>` from `POST /api/login`, `/api/setup` or `/api/recover`.
- **Access levels:**
  - **Public**: no token.
  - **User**: any signed-in user.
  - **Admin**: role admin.
  - **Local**: only from the main PC itself (loopback), so another device on the Wi-Fi can't claim or restore a store.
- **Errors:** non-2xx responses carry:
  ```json
  { "error": "insufficient_stock", "message": "Only 12 Tab of Panadol in stock.",
    "reason": "stock.short", "params": { "name": "Panadol", "have": 12 } }
  ```
  | `error` | HTTP | Meaning |
  |---|---|---|
  | `bad_request` | 400 | Invalid input. |
  | `unauthorized` | 401 | Missing or expired session. Sign in again. |
  | `forbidden` | 403 | Not allowed for this role or place. |
  | `approval_required` | 403 | Retry with a manager approval. |
  | `not_found` | 404 | |
  | `conflict` | 409 | Duplicate, wrong state, cloud belongs to another store… |
  | `insufficient_stock` | 409 | Not enough unexpired stock. |
  | `locked` | 429 | Too many wrong PINs. Wait the given minutes. |
  | `internal` | 500 | Database or unexpected error (details only in the server log). |

  `reason` and `params` let the UI show the message in the user's language (`src/i18n/errors.ts`).
- **Idempotency:** `POST /api/sales`, `/api/returns`, `/api/purchases` and `/api/transfers` accept a `clientRef`. Resending the same one returns the original result.
- **CORS:** only the Tauri origins (`tauri://localhost`, `http(s)://tauri.localhost`) and `http://localhost:*` / `http://127.0.0.1:*`.

### Endpoints

| Method & path | Access | Purpose |
|---|---|---|
| `GET /api/health` | Public | `{ ok, setUp, store, version }`; tills poll it every 15 s. |
| `POST /api/setup` | Public + Local | Create the store and owner (first run only). |
| `GET /api/login-tiles` | Public | Users for the sign-in screen (no secrets). |
| `POST /api/login` · `POST /api/logout` | Public · User | Start / end a session. |
| `GET /api/me` · `PATCH /api/me/language` | User | Current user; save their UI language. |
| `GET/POST /api/users` · `PUT /api/users/{id}` | Admin | Users (role, PIN/password, can approve, language, active). |
| `GET /api/settings` · `PUT /api/store` | Admin | Store details, receipt text and language. |
| `POST /api/recovery-code` · `POST /api/recover` | Admin · Public + Local | Make a one-time owner recovery code; reset the password with it. |
| `GET /api/products/search?q=` | User | Name/formula/company search with stock and prices. |
| `GET /api/products/barcode/{code}` | User | Product and pack for a barcode. |
| `GET /api/catalogue` | User | Full catalogue for offline selling; records a price list. |
| `POST /api/products` · `PUT /api/products/{id}` | Admin | Create / edit a product (packs, barcodes, opening stock). |
| `POST /api/products/import` | Admin | Validate (`commit:false`) or import (`commit:true`) Excel rows. |
| `POST /api/stock/receive` · `POST /api/stock/adjust` | Admin | Add stock to a batch; correct a batch with a reason. |
| `GET /api/inventory` · `GET /api/inventory/{id}` | Admin | Inventory list with filters; product detail, batches, history. |
| `GET /api/shifts/current` · `POST /api/shifts/open` | User | This till's shift; open one with opening cash. |
| `GET /api/shifts/{id}/summary` · `POST .../count` · `POST .../close` | User | Blind count, recount and close (Z-report data). |
| `POST /api/cash-moves` | User | Cash in / out of the drawer. |
| `POST /api/sales` · `GET /api/sales/{id}/receipt` | User | Make a sale (online or offline-replayed); fetch a receipt. |
| `GET /api/returns/sale/{receiptNo}` · `POST /api/returns` · `GET /api/returns/{id}/slip` | User | Find a sale to return; refund; refund slip. |
| `GET/POST /api/customers` · `PUT /api/customers/{id}` | User · Admin | Customers (the till can add one; the admin edits limits etc.). |
| `GET /api/rules` · `PUT /api/rules` | User · Admin | Approval rules. |
| `POST /api/actions` | User | Record a till action (item removed, bill cancelled, drawer opened) with its approval. |
| `GET /api/activity?date=&filter=` | Admin | Activity log. |
| `GET /api/dashboard?from=&to=` | Admin | Dashboard figures. |
| `GET /api/reports/{name}?from=&to=&…` | Admin | One of the reports below. |
| `GET/POST /api/suppliers` · `PUT /api/suppliers/{id}` | Admin | Suppliers. |
| `GET /api/suppliers/{id}/ledger` · `POST .../payments` · `POST .../opening` | Admin | Supplier ledger, payments, old balance. |
| `GET/POST /api/purchases` · `GET /api/purchases/{id}` · `GET /api/purchases/last-buy/{productId}` | Admin | GRNs; last trade price for a product. |
| `GET/POST /api/expenses` · `DELETE /api/expenses/{id}` | Admin | Expenses by month. |
| `GET /api/khata` · `GET /api/khata/{id}` · `POST .../payments` · `POST .../opening` | Admin | Customer balances, statement, payment, old balance. |
| `GET /api/tills/open` | Admin | Tills with an open shift (for "cash goes into drawer"). |
| `GET/POST /api/counts` · `GET /api/counts/{id}` · `PUT .../lines/{line}` · `POST .../products` · `POST .../apply` · `POST .../cancel` | Admin | Stock counts. |
| `GET/POST /api/transfers` · `GET /api/transfers/{id}` · `GET /api/transfers/incoming` · `POST /api/transfers/receive` | Admin | Branch transfers. |
| `GET/PUT/DELETE /api/cloud` · `POST /api/cloud/sync` | Admin | Cloud backup status, enable, disable, sync now. |
| `POST /api/backups` | Admin | Make a local backup file now. |
| `POST /api/restore` | Public + Local | Rebuild an empty store from its cloud copy. |
| `GET /api/hq` · `POST /api/hq/stores` · `DELETE /api/hq/stores/{id}` | Admin | Head-office figures; add/remove another store. |
| `GET/PUT /api/fbr` · `POST /api/fbr/send` | Admin | FBR settings and status; send waiting invoices now. |

Report names for `/api/reports/{name}`: `profit_loss`, `product_profit`, `discounts`, `daily_sales`, `sales_by_product`, `sales_by_category`, `sales_by_cashier`, `sales_by_payment`, `returns`, `shifts`, `cash_book`, `stock_valuation`, `stock_movement`, `expiry`, `slow_stock`, `purchase_register`, `payables`, `receivables_aging`, `sales_tax`.

Tests cross-check that the reports agree with each other and with the dashboard: daily sales and sales by cashier sum to net sales, product profit sums to P&L gross profit, the cash book sums to expected drawer cash, and stock valuation equals inventory value.

---

## 10. Authentication and security

| Topic | Implementation |
|---|---|
| Secrets | Argon2 (`password-hash`). Cashier PIN = exactly 4 digits; admin password ≥ 8 characters. |
| Sessions | Random token kept **in server memory** (`Auth`), with a 16-hour hard limit. Restarting the main PC signs everyone out. The UI keeps the token in memory only. |
| Lockout | 5 wrong tries → locked for 5 minutes (sign-in, approvals, recovery code). Lockouts are audited. |
| Roles | `AdminUser` extractor for backoffice routes; `AuthUser` for till routes. |
| First-run, recovery and restore | `LocalOnly`: refused unless the request comes from the main PC's loopback address. |
| Recovery code | `XXXX-XXXX-XXXX`, shown once, stored only as a hash, single use. A new code replaces the old one. |
| Audit | `audit_log` is append-only, enforced by triggers. It records approvals, refunds, cash moves, corrections, counts, shift closes, price changes, lockouts and settings changes. |
| Cloud secrets | Cloud token, head-office tokens and FBR token are local-only settings, excluded from sync, so a read-only head-office token can't reveal them. |
| Offline abuse | Offline sales are accepted only at prices from a price list this PC actually issued, within the discount limit, and timed between 10 minutes in the future and 7 days ago. Shortfalls and over-limit discounts are flagged in the activity log. |
| Desktop surface | Tauri CSP limits network calls to `self`, IPC and `:7420`. `open_whatsapp` accepts only `https://wa.me/` links. `save_download` accepts a bare file name only, never a path. |
| Network | The API is plain HTTP on the shop LAN. Keep the shop Wi-Fi private, and open port 7420 only on the private network profile. |

---

## 11. Offline selling on extra tills

1. Every 5 minutes an extra till downloads `GET /api/catalogue`: products, packs, barcodes, prices, and a `priceListId`. The server stores each list it hands out in `offline_price_lists`.
2. The till checks `GET /api/health` every 15 s. When it fails, the till switches to offline mode: a banner, search from the cached catalogue, and the payment methods cash, card, JazzCash and Easypaisa. Khata, returns, cash moves and PIN-protected actions wait for the main PC.
3. Offline receipts are numbered `<store>-<till>-X<n>`. The counter is the highest of the local counter, the server's last X number and the queue, so numbers never repeat.
4. Each sale is queued with its full input, the prices and line totals shown, `createdAt`, `clientRef` and `priceListId` (`src/domain/offline.ts`). If `localStorage` is unavailable, the queue lives in memory and the till warns that closing it would lose sales.
5. When the main PC answers, the queue is sent oldest first as normal `POST /api/sales` calls with an `offline` block. The server:
   - checks the receipt number, the time window and that the lines add up, and the prices against the referenced price list;
   - records the sale **at the till's time and prices**, even if the shift has since closed;
   - allocates whatever stock exists and flags shortfalls and over-limit discounts in the activity log instead of refusing.
6. A sale the server refuses is moved to `medstore.offline.refused` with the reason, so nothing is silently lost. `401` re-prompts for sign-in, and `5xx` keeps the sale queued.
7. A shift can't be closed while that till still has unsent offline sales.

---

## 12. Cloud backup, restore and multi-store

### Turso backup

- **One Turso database per store.** The cloud copy has the same tables and rows as the shop's, plus `_sync_meta` (store code, store name, schema version).
- **Change tracking:** `cloud::sync::ensure_tracking` creates `AFTER INSERT/UPDATE/DELETE` triggers on every tracked table. While `cloud_enabled = 1`, they write `(table, primary-key JSON)` into `sync_queue`. Triggers are rebuilt at every start, so a changed private-settings list reaches old databases too.
- **Excluded:**
  - Local tables: `sync_queue`, `sqlite_sequence`, `offline_price_lists`.
  - Private settings: `cloud_%`, `hq_%`, `fbr_token`.
- **Upload:**
  - The background loop runs every **30 s**, or at once on *Sync now*.
  - It reads up to **400** queued keys, fetches the current row for each and sends upserts (or deletes) in **one transaction**, then removes those queue entries.
  - A row changed many times is sent once. `audit_log` rows are never deleted in the cloud.
- **Protocol:** `cloud/hrana.rs` speaks Turso's *Hrana over HTTP* (`POST /v2/pipeline`) with `reqwest` + rustls. URLs: `libsql://…` or `https://…` (plain `http://` only for localhost in tests).
- **Enabling:**
  - The server checks `_sync_meta`. A database holding **another store's** code is refused, and one made by a **newer schema** is refused.
  - It then applies any missing migrations remotely, records the store identity and queues every row for the first upload.
- **Status:** `GET /api/cloud` returns the last sync, pending count and last error. The backoffice menu shows "Cloud backed up · 14:31" or "Offline · N changes waiting".

### Restore onto a new main PC

`POST /api/restore` (first-run screen, Local only) reads every table from the cloud into a fresh **in-memory** database first, so a dropped connection leaves the PC untouched. It then writes it to disk and keeps syncing to the same cloud database. Users, PINs, stock, sales and khata all come back. Shifts left open on the old PC stay open for the owner to close.

### Daily backup files

`cloud/backup.rs`: once per day (checked by the background loop) the server runs `VACUUM INTO backups/store-YYYY-MM-DD-HHMMSS.sqlite`, keeping the newest **14**. It works with no internet. `POST /api/backups` makes one on demand. To restore one: close the app, replace `store.sqlite` with the copy (and remove `store.sqlite-wal` / `-shm`), then start the app.

### Head office (All stores)

The owner adds other stores' cloud URLs with **read-only** tokens (`hq_stores`). `GET /api/hq` runs the dashboard sums directly against each store's cloud copy, alongside this store's local figures, and reports when each store last synced. The same list lets `GET /api/transfers/incoming` find transfer notes other stores have sent to this one.

### Setting up Turso

```bash
turso db create store-b1
turso db show store-b1 --url                   # libsql://store-b1-<org>.turso.io
turso db tokens create store-b1                # read-write token for B1's main PC
turso db tokens create store-b1 --read-only    # for head office / other branches
```

Paste the URL and read-write token in *Backoffice › Store, Users & Settings › Cloud backup*. Keep both somewhere safe: they are what a replacement PC needs.

---

## 13. FBR invoice reporting

For Tier-1 retailers. It is **off by default** and set up per store (`fbr.rs`).

- When enabled, each sale and return is added to `fbr_queue` **in the same transaction** that records it. The background loop posts it to PRAL. A sale never waits on FBR for more than a few seconds.
  - Sandbox: `https://esp.fbr.gov.pk:8244/FBR/v1/api/Live/PostData`
  - Live: `https://gw.fbr.gov.pk/imsp/v1/api/Live/PostData`
  - Or a custom URL.
- The request is built from the stored document: POS ID, buyer, items with the PCT code (default `30049099`, medicines), tax and totals. It is sent with the Bearer token and a 15 s timeout.
- On success, FBR's invoice number is stored on the sale/return and printed with a QR code. On failure the attempt count grows and the retry waits `2^attempts` minutes (capped at 64). Settings shows the queue and the last error. `POST /api/fbr/send` sends at once.
- **Status:** built to PRAL's published format and tested against a stand-in server only. Validate with a sandbox POS ID before going live.

---

## 14. Printing

```
Receipt JSON ─► print/doc.ts (Doc: text rows, rules, barcode, QR, totals)
                   ├─► DocView.tsx        on-screen preview (exactly what prints)
                   └─► raster.ts          canvas → 1-bit image, 576 / 384 dots wide,
                                          split into ≤ 8000-dot pages
                          └─► Tauri print_raster ─► pos-printer::raster_job
                                 ESC @ · GS v 0 (raster) · feed · GS V (cut) · ESC p (drawer)
                                 └─► windows_spooler::send_raw (RAW datatype, no dialog)
```

- Printing receipts **as images** means Urdu prints correctly on any ESC/POS printer, even one with no Urdu font.
- Documents: sale receipt, refund slip, Z-report, khata payment slip and statement, transfer slip, test page. Each can be in English, Urdu or both.
- The sale barcode is **Code128** (`print/code128.ts`) and is scannable on the returns screen. The FBR QR code uses `qrcode-generator`.
- Printer settings are per till (`device.json`). The drawer must be cabled to the printer's DK port; the pulse is sent on pin 2. Paper heights above `MAX_HEIGHT` (200,000 dots) are refused.
- In a browser, or with no printer chosen, `window.print()` and the system dialog are used.

---

## 15. Frontend

- **Boot (`App.tsx`):**
  1. `loadRuntime()` reads the device config through Tauri, or uses the browser defaults.
  2. `GET /api/health` decides between device setup, store setup and sign-in.
  3. Admins land in the **Backoffice**, cashiers in the **Till**.
- **State:** React state per screen. There is no global store. The API client is a thin typed `fetch` wrapper (`api/client.ts`) that adds the Bearer token and turns error bodies into `ApiError`.
- **Domain mirror (`src/domain`):**
  - Pricing, cart totals, money parsing (Urdu digits, lakh grouping) and the offline queue.
  - Purchase maths, stock display ("15 Strip + 8 Tab"), dates, barcodes, the Excel import sheet and WhatsApp links.
  - All of it is unit-tested with Vitest.
- **i18n:**
  - `i18n/*.ts` hold English and Urdu dictionaries per area. The Urdu object is typed `Record<keyof typeof en, string>`, so a missing key fails type-checking. `coverage.test.ts` checks that every `t('…')` key used in the source exists.
  - Urdu switches the document to `dir="rtl"`. Numbers, codes and receipt numbers are wrapped with the `ltr()` bidi helper.
  - Each user's language is saved on the server (`PATCH /api/me/language`).
- **Errors:** `i18n/errors.ts` maps server `reason` codes plus `params` to English or Urdu messages, falling back to the server's `message`.
- **Downloads:** `platform.saveFile(name, blob)` calls the Tauri `save_download` command on the desktop (saving to Downloads and showing the file), or does an anchor download in a browser.
- **Styling:** `styles.css` uses design tokens taken from the Bizflow logo.

  | Token | Value | Use |
  |---|---|---|
  | `--brand` | `#076599` | Main brand colour |
  | `--brand-dark` | `#054e77` | Hover, pressed and dark accents |
  | `--brand-soft` | `#e3f2fb` | Light brand backgrounds |
  | `--night` | `#0b2236` | Dark surfaces (menu, top bar) |
  | `--focus` | `#5cc8f0` | Focus ring |

  Fonts: IBM Plex Sans / Mono and Noto Nastaliq Urdu.

---

## 16. Desktop shell (Tauri)

| Command | Purpose |
|---|---|
| `get_runtime_info` | Device config and the API base URL. |
| `save_device_config` | Save mode / main PC address / till code (keeps printer settings). |
| `restart_app` | Restart after setup changes (e.g. becoming the main PC starts the server). |
| `save_printer_settings`, `list_printers` | Per-till printer configuration; list Windows printers. |
| `print_raster` | Print a rasterised document (base64 1-bit image) with cut/drawer options. |
| `open_cash_drawer` | Send the drawer pulse. |
| `open_whatsapp` | Open a `https://wa.me/…` link in the default browser. |
| `save_download` | Save bytes to the Downloads folder without overwriting; reveal it in Explorer. |

On a main PC, `setup()` spawns `pos_server::run` with the database at `<app data>/store.sqlite`, host `0.0.0.0` and the configured port. The window is `1366×800` (min `1024×640`) and starts maximised. Bundling targets **NSIS**, per-machine install.

---

## 17. Testing and CI

### Local checks

```bash
npm run typecheck && npm test && npm run build          # TypeScript, Vitest, production bundle
cargo test --workspace --exclude medstore-pos           # server + printer tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

### What is covered

| Suite | Covers |
|---|---|
| `crates/pos-server/tests/sales.rs` | FEFO, expiry, weight items, discounts, approvals, khata limits, retries, offline replay and price-list checks |
| `returns_shifts.rs` | Returns, refund limits, drawer cash, blind count, recount, close |
| `backoffice.rs` | Products, imports, purchases with bonus/freight, suppliers, expenses, khata, settings, recovery |
| `reports.rs` | Every report, and the cross-checks between reports and the dashboard |
| `stock_ops.rs` | Counts during selling, transfers (send/receive/idempotency/new products) |
| `cloud.rs` | Change tracking, upload batches, ownership and schema checks, restore, head office (against a stand-in Hrana server) |
| `fbr.rs` | Invoice building, queueing, retries, receipt numbers (against a stand-in PRAL) |
| `api.rs` | End-to-end till flow over HTTP, admin-only routes, error format with reason/params |
| `qa_regressions.rs` | Regression tests for issues found in QA rounds |
| `src/**/*.test.ts` | Pricing parity with Rust, cart, money parsing, offline queue, barcode/Code128, raster, import sheet, purchase maths, WhatsApp, i18n coverage, error translations |

Integration tests use `db::open_in_memory()` and call handlers in-process, with helpers in `tests/common`.

### CI (`.github/workflows/ci.yml`)

Runs on every push and pull request:
- **Ubuntu:** `npm ci` → typecheck → Vitest → build → `cargo fmt --check` → `cargo test` → `cargo clippy -D warnings`.
- **Windows:** clippy and tests for `pos-printer` (the spooler code only compiles on Windows).

---

## 18. Building and releasing

```bash
npx tauri build      # → target/release/bundle/nsis/Bizflow POS_<version>_x64-setup.exe
```

- **Workflow:** *Windows installer* (`.github/workflows/windows-installer.yml`) runs on demand from the Actions tab, or on tags `v*`. It uploads the artifact `bizflow-pos-windows`.
- **Versioning:** bump `version` in `package.json`, `src-tauri/tauri.conf.json` and the crates' `Cargo.toml`, then tag `vX.Y.Z`. Schema changes ship as new migrations and apply automatically at start-up. The cloud refuses to accept a backup from an app older than its schema.
- **Code signing:** the installer is currently **unsigned**, so Windows SmartScreen shows *More info → Run anyway*. To sign, add a certificate (or Azure Trusted Signing) and set `bundle.windows.certificateThumbprint` / `signCommand` in `tauri.conf.json`.
- **Release profile:** `opt-level=3`, LTO, one codegen unit, stripped, **overflow checks on**.
- **User manual:** `node docs/manual/build.cjs` rebuilds `docs/Bizflow-POS-User-Manual.pdf`. It needs Playwright Chromium, `pdftotext` (poppler) and Python `pypdf`. It renders twice so the contents get real page numbers. Screenshots live in `docs/manual/img/`.

---

## 19. Deployment and operations

### Installing in a shop

1. Install on the **main PC**:
   - Choose *This is the main PC*.
   - Create the store: the store code is permanent and must be unique per branch.
   - Give the PC a **fixed LAN IP** in the router.
2. Allow inbound TCP **7420** on the main PC's firewall (Private profile).
3. On each extra counter: install, choose *This is an extra till*, enter the main PC's IP and a unique till code (`T02`, `T03`…).
4. As the owner, on each till: *Printer* → choose the printer and paper width → test print and drawer.
5. Optional: enable cloud backup (section 12) and FBR (section 13). Make and print the owner recovery code.

### Routine operations

| Task | How |
|---|---|
| Update the app | Run the new installer over the old one; data and settings are kept and migrations run on start. Back up first. |
| Back up | Automatic daily file + continuous cloud. On demand: *Settings › Back up now*. Copy a file to USB regularly. |
| Replace the main PC | With cloud: new install → *Restore from cloud backup*. Without: copy the latest backup file into the new app data folder as `store.sqlite`. |
| Uninstall completely | Uninstall from *Settings › Apps*, then delete `%APPDATA%\pk.medstore.pos` (this deletes the store database: back it up first). |
| Forgotten owner password | On the main PC: *Forgot password?* + recovery code. |
| Forgotten cashier PIN | Owner: *Users › Edit* → new PIN. |

### Troubleshooting

| Symptom | Check |
|---|---|
| Till shows "Can't reach the main PC" | The main PC is on and the app is open; both PCs are on the same network; the IP hasn't changed; port 7420 is open. The till keeps selling offline meanwhile. |
| Cloud shows changes waiting | Internet; the token is still valid (`turso db tokens create` for a new one); the last error in *Settings › Cloud backup*. |
| "That cloud database holds store X's backup" | Each store needs its own Turso database. |
| Nothing prints / drawer doesn't open | The printer is installed in Windows and chosen in *Printer*; paper width; the drawer cable is in the printer's DK port. |
| FBR invoices stuck | *Settings › FBR*: last error; POS ID / token / mode; *Send waiting invoices now*. |
| Server log | Run `pos-server` from a console against a copy of the database to see `stderr` messages. |

---

## 20. Extending the system

| Change | Steps |
|---|---|
| **Schema** | Add `migrations/009_<name>.sql`, register it in `db::MIGRATIONS`. New tables are tracked for cloud sync automatically; add a table to `LOCAL_TABLES` if it must stay on the PC, or a setting key to `PRIVATE_SETTINGS` if it is secret. |
| **Endpoint** | Write logic in the relevant module (taking `&Connection` / `&mut Connection`), add a handler in `api.rs` with the right extractor (`AuthUser` / `AdminUser` / `LocalOnly`), register the route, add the client call in `src/api/client.ts` and types in `types.ts`, and add an integration test. |
| **Business error** | Return `AppError::…(msg).because("area.reason", json!({…}))` and add English + Urdu text for the reason in `src/i18n/errors.ts`. |
| **Report** | Add a function in `reports.rs`, a name in the `match`, the label/columns in `src/i18n/reports.ts` and the menu entry in `backoffice/Reports.tsx`; add a cross-check test in `tests/reports.rs`. |
| **UI text** | Add the key to the English object and its Urdu translation (the type forces it); use `t('key', { params })`. |
| **Money maths** | Change `pricing.rs` **and** `src/domain/pricing.ts` together and add the same case to both test suites. |
| **Printed document** | Build it with the `Doc` helpers in `print/doc.ts`; it previews with `DocView` and prints through `printDoc`. |

Coding conventions:
- Money is paisa as `i64` / integer `number`.
- Stock is in base units.
- Timestamps come from `db::now()` and ids from `db::new_id()`.
- Every sensitive action calls `db::audit`.
- Multi-table writes run in one transaction.
- Code is formatted with `cargo fmt` and must pass clippy with no warnings.

---

## 21. Known limitations

- The API is HTTP (not HTTPS) on the shop LAN. The LAN is assumed to be private.
- Sessions are in memory, so restarting the main PC signs everyone out.
- Only extra tills can sell offline. If the **main PC itself** is down, the extra tills carry on, but returns, khata and backoffice work wait for it.
- Turso is the only cloud backend.
- The FBR integration has not been certified against PRAL's live service.
- The Windows installer is not code-signed.
- Some rare server messages have no Urdu translation and show in English.
- The sales tax summary uses each product's current tax rate (Pakistani retail prices are tax-inclusive).

---

## 22. Feature history

| Milestone | Delivered |
|---|---|
| **1. Foundation + till** | Store server, SQLite schema, batches/FEFO, weight items, payments incl. khata, PIN sign-in with lockout, safe retries; React till with Urdu RTL; Tauri main-PC / extra-till modes. |
| **2. Daily control** | Returns to original batch, blind shift close and Z-report, cash in/out, approval rules, append-only activity log, direct ESC/POS raster printing with cash drawer, Urdu server messages. |
| **3. Backoffice** | Dashboard, inventory and corrections, product form, Excel import, GRNs with bonus/freight, supplier payables, expenses, khata with WhatsApp reminders, users, store settings, recovery code. |
| **4. Cloud** | Turso backup per store, restore onto a new PC, daily backup files, head-office view. |
| **5. Reports** | 19 financial and stock reports with print/PDF and Excel export, cross-checked by tests. |
| **6. Extras** | Stock counts, branch transfers, offline selling on extra tills with price-list verification, optional FBR reporting. |
| **Since** | QA hardening (offline abuse, approval lockouts, long-receipt paging, stricter product validation), receipt returns-policy text and Code128 receipt barcode, Bizflow branding and colour scheme, desktop downloads, sample medicine list, user manual. |
