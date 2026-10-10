# Medical & General Store POS

Point-of-sale for a medicine and general store in Pakistan. It is built to work fully offline, and each store has its own stock. It has an English / اردو interface and prints 80 mm receipts.

UI design (all screens): https://claude.ai/artifact/DdLH1Rp4aDxSGTM2m2EKUz

## What works now (milestone 1: foundation + Till)

- **Store server** (`crates/pos-server`): the store's SQLite database and the sale rules, served to every till in the shop over the local network.
  - **Batches:** stock is kept per batch and sold earliest-expiry first. Expired stock is never sold, and each batch keeps the price printed on its pack.
  - **Selling:** medicines can be sold by Box / Strip / Tablet. Weight items (sugar etc.) can be sold by **amount (Rs 10 of sugar)** or **grams**.
  - **Money:** prices include sales tax, and bill discounts are supported. Payments can be Cash (with change), Card, JazzCash, Easypaisa or Khata (udhaar) with a credit limit.
  - **Manager PIN:** needed for a discount above 5% and for khata over the customer's limit. Approvals go into an append-only activity log.
  - **Sign-in:** cashiers use a 4-digit PIN and the owner uses a password. A user is locked out for 5 minutes after 5 wrong tries.
  - **Safe retries:** if a till's connection drops while paying, it can resend the sale and it is still only charged once.
  - **Cloud backup:** every sale and stock change is queued in an `outbox` table for cloud backup. The upload itself comes in milestone 4.
- **Till UI** (`src/`, React + TypeScript):
  - First-run setup of the computer and of the store.
  - PIN sign-in, where each user's language is remembered.
  - Opening a shift with cash in the drawer.
  - Search by name, formula or barcode (scanner + Enter).
  - Pack switching, the sugar Rs/grams editor, discounts, payment methods, change, and a khata customer picker.
  - Manager-approval dialog.
  - 80 mm receipt in English, Urdu or both.
  - Full Urdu right-to-left mode.
- **Desktop app** (`src-tauri`, Tauri 2):
  - On first start you choose whether the PC is the **main PC** (it runs the store server and database) or an **extra till** (it connects to the main PC's address).

Not built yet: backoffice screens, returns, shift close, stock count, transfers, reports, cloud sync, silent ESC/POS printing. See the roadmap below.

## How it fits together

```
 Store (one shop)                                        Cloud (later)
 ┌──────────────────────────────────────────────┐
 │ Main PC                                      │
 │  Desktop app ── store server ── SQLite DB    │  outbox ──►  Turso (one database
 │                    ▲  :7420                  │              per store + head office)
 │ Extra till         │  shop LAN / Wi-Fi       │
 │  Desktop app ──────┘                         │
 └──────────────────────────────────────────────┘
```

- Each store has its own database on its main PC, so a store keeps selling with no internet.
- Money is stored as whole paisa, and stock in the smallest unit (tablet, piece, gram).
- The till never decides prices. The server recomputes every line, and the till's copy of the maths (`src/domain/pricing.ts`) is only for showing totals while scanning. Both copies are tested with the same cases.

## Try it in a browser (any OS)

You need Node 22 and Rust (stable).

```bash
npm install
npm run server:demo      # terminal 1: store server with demo data on :7420
npm run dev              # terminal 2: open http://localhost:5173
```

Demo sign-ins:

| User | Role | Login |
|---|---|---|
| Ali | Cashier | PIN 1234 |
| Sana | Cashier, can approve, Urdu | PIN 2345 |
| Usman | Cashier | PIN 3456 |
| Owner | Admin | password `owner1234` |

The demo products and prices are samples. Try:
- scanning the Panadol strip barcode `8964000123463`;
- selling "Rs 10" of sugar;
- a 10% discount, which needs Sana's PIN;
- 15 strips of Flagyl, which is refused because part of its stock is expired.

## Run the desktop app (Windows)

Prerequisites: [Rust](https://rustup.rs), Node 22, and the "Desktop development with C++" workload from Visual Studio Build Tools. WebView2 is already part of Windows 10/11.

```bash
npm install
npx tauri dev
```

On first start, choose **This is the main PC**. To load the demo store, set `MEDSTORE_DEMO=1` before starting.

To set up a second till in the same shop:
1. On it, choose **This is an extra till** and enter the main PC's IP address (e.g. `192.168.1.10`).
2. Allow port **7420** through the main PC's Windows firewall.

### Installer

`npx tauri build` produces an `.exe` installer in `target/release/bundle/nsis/`. Or run the **Windows installer** workflow from the GitHub Actions tab and download it from the run's artifacts.

### Where the data lives

`%APPDATA%\pk.medstore.pos\store.sqlite` on the main PC. Until cloud sync is added, copy that file (with the app closed) to a USB drive to back up.

### Printing

The receipt is laid out for 80 mm paper and printed through Windows' print dialog:
1. Choose the thermal printer.
2. Set the paper to 80 mm.
3. Tick "remember".

Silent printing (no dialog, opens the cash drawer) is planned for milestone 2. Urdu receipts will be sent to the printer as an image so they work on any thermal printer.

## Project layout

```
crates/pos-server/   Store server: database, migrations, sale rules, HTTP API (Rust, axum, rusqlite)
  migrations/        SQL schema
  tests/             End-to-end sale and API tests
src/                 Till UI (React + TypeScript)
  api/               API client and types
  domain/            Money maths and cart (mirrors the server)
  i18n/              English / Urdu text
  screens/           Device setup, store setup, sign-in, open shift, Till
src-tauri/           Desktop shell: main-PC / extra-till mode, starts the store server
```

## Checks

```bash
npm run typecheck && npm test && npm run build
cargo test -p pos-server
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI runs all of these on every push.

## Roadmap

1. **Foundation + Till** — done.
2. **Daily control:**
   - Returns / refunds and Close shift (blind cash count, Z-report).
   - Manager-approval rules screen and the activity log viewer.
   - Silent ESC/POS printing with Urdu printed as an image, and opening the cash drawer.
   - Translating server messages into Urdu.
3. **Backoffice:**
   - Dashboard, Inventory, Add/Edit product with Excel import, and Purchases (GRN) with supplier payables.
   - Expenses, Customers & Khata, users and settings.
   - Paid add-ons (SMS OTP, WhatsApp API), off by default.
4. **Cloud:**
   - Turso sync from the outbox (one database per store), with a nightly full backup and restore.
   - Head-office view of all stores.
5. **Reports:** P&L, sales by product / category / cashier / payment, stock valuation, expiry, khata aging, and the sales tax summary.
6. **Extras:**
   - Stock count and branch transfers.
   - Optional FBR POS integration (off by default, per store).
   - An extra till keeps selling if the shop network drops, then catches up.
