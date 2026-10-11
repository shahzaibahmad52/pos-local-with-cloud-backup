# Medical & General Store POS

Point-of-sale for a medicine and general store in Pakistan. It is built to work fully offline, and each store has its own stock. It has an English / اردو interface and prints 80 mm receipts.

UI design (all screens): https://claude.ai/artifact/DdLH1Rp4aDxSGTM2m2EKUz

## What works now

### Milestone 5: financial reports

Backoffice › **Financial reports**: 19 reports for today, yesterday, the last 7 days, this month, last month or any dates. Each one can be printed on A4 (or saved as PDF from the print dialog) and exported to Excel, in English or Urdu.

- **Profit:**
  - **Profit & loss:** gross sales, discounts, returns, sales tax, cost of goods (from the batches actually sold), gross profit and margin. Then expenses by category, stock written off (damaged / expired / lost), and net profit.
  - **Gross profit by product** and **discounts given** (with who approved them).
- **Sales:**
  - Daily summary with cash / card / wallets / khata.
  - By product, by category, by cashier (including cash short/over from shift counts), and by payment method.
  - Returns & refunds.
- **Cash:** every shift close (Z-report) and a cash book of everything that went in or out of the drawers.
- **Stock:**
  - Valuation at cost and at sale price.
  - Movement (opening, received, sold, returned, corrected, closing).
  - Expiry within 1/3/6/12 months.
  - Dead / slow stock with no sale in 30–180 days.
- **Purchases:** purchase register and supplier payables.
- **Customers:** khata receivables with ageing (0–30, 31–60, 61–90, 90+ days).
- **Tax:** sales tax summary by rate, net of refunds.
- **Branch comparison:** links to the All stores page.

The reports are tested to agree with each other and with the dashboard:
- Daily sales and sales by cashier add up to net sales.
- Product profit adds up to the P&L gross profit.
- The cash book adds up to the drawer's expected cash.
- Stock valuation equals the inventory value.

### Milestone 4: cloud backup

- **Cloud backup to Turso (free plan), one database per store:**
  - Every change on the main PC (sales, stock, khata, expenses, users…) is copied to the store's own cloud database. The cloud copy has the same tables and rows as the shop's.
  - Sales are always saved on the main PC first. Changes go up every 30 seconds while the internet is on, and simply wait while it is off.
  - The backoffice menu shows the state, e.g. "Cloud backed up · 14:31" or "Offline · 41 changes waiting". Settings shows the last upload, what is waiting, the last error, and **Sync now**.
  - The cloud token stays on the main PC; it is never copied to the cloud.
  - A cloud database already holding another store's backup is refused, so two stores can't overwrite each other.
- **Restore on a new main PC:** on the first-run screen choose **Restore from cloud backup**, paste the store's URL and token, and the whole store comes back: products, stock, sales, khata, users and their PINs. Backup carries on to the same database.
- **Daily backup files:** once a day the main PC writes a copy of the database to a `backups` folder next to it, keeping the last 14. **Back up now** makes one at any time.
- **All stores (head office):** add each other store's cloud database once (URL + a read-only token). The page shows sales, gross profit, expenses, net profit and khata for every store and all stores together, with when each store last synced. The figures use the same sums as the dashboard.

### Milestone 3: backoffice (owner / admin only)

The owner signs in to the **backoffice**; cashiers go straight to the till. The owner can open the till from the backoffice and come back with the **Backoffice** button in the till header.

- **Dashboard:** sales after refunds, gross profit (sales tax and cost of goods taken off), expenses and net profit for today, the last 7 days or this month. Also a 14-day sales chart, how customers paid, khata to collect, what is owed to suppliers, low stock and batches expiring within 90 days.
- **Inventory:**
  - Every product with stock (counted in strips, boxes or kg), average cost, sale price, next expiry and stock value at cost.
  - Filters for low stock, expiring soon, expired stock on the shelf, medicines, general items and archived products; search by name, formula, company or barcode.
  - Each product's batches with cost and printed prices, and a **Correct** button for breakage, expired stock thrown away, loss or a recount. Every correction goes into the activity log.
- **Add / edit product:** pack levels and prices, Urdu pack names, barcodes (or an in-store barcode), tax, Rx / controlled / fridge flags, reorder level, price per kg for loose items, and opening stock for a new product. Archived products disappear from the till but keep their history.
- **Import from Excel:** download the template, fill one row per product, choose the file and check the preview (new / price update / needs fixing) before importing. Rows with problems can be downloaded with the reason next to each.
- **Purchases (GRN):**
  - Scan or search products onto the purchase. The last trade price is filled in.
  - Batch, expiry, qty, **bonus** (free packs), trade price, discount and MRP per line. Freight is spread over the lines, so each batch's cost includes bonus units and freight.
  - The MRP becomes that batch's printed price at the till.
  - Paid now from the owner's cash, bank, wallet or a till's drawer; the rest goes on the supplier's account with a due date.
  - Purchase history with every line.
- **Suppliers & payables:** balances, due dates, overdue flags, a ledger per supplier, payments and old balances carried over from before the app.
- **Expenses:** electricity, generator fuel, rent, salaries, etc. with a monthly total per category and Excel export. Paid from a till's drawer, an expense counts in that shift's cash.
- **Customers & khata:**
  - Balances with ageing (days since the oldest unpaid bill), overdue and near-limit counts, money collected this month.
  - Ledger with running balance; receive a payment (cash can go into a till's drawer) and print a payment slip; print an 80 mm statement.
  - **WhatsApp reminder** opens WhatsApp with the message typed in the customer's language; you press Send, so it's free.
  - Old balances from the paper register, credit limit, receipt language.
- **Store, users & settings:** store details and receipt footer in English and Urdu, default receipt language, adding and editing users (PIN / password reset, approver, language, switch off), and the **owner recovery code** for a forgotten password (sign-in screen › Forgot password?, main PC only).
- **Paid add-ons** (SMS OTP, automatic WhatsApp, FBR) are shown switched off with the free option in use, ready to connect when you choose a provider.

### Milestone 2: daily control

- **Returns / refunds** (<kbd>F9</kbd>):
  - Scan the receipt barcode (or type the receipt number). Choose quantity, reason and condition per item.
  - Sealed items go back into **their original batch**. Damaged or expired items are written off instead.
  - The refund is the item's share **after the bill's discount**. It can be given as cash, khata credit, card or wallet.
  - Fridge items are refused, refunds above Rs 500 need a manager PIN, and a sale can never be refunded more than was paid.
- **Close shift with a blind count:**
  - The cashier types how many of each note and coin are in the drawer. The expected amount stays hidden until they submit.
  - Expected cash = opening cash + cash sales + cash in − cash out − cash refunds.
  - One recount is allowed. A difference over Rs 100 needs a note.
  - Closing prints a Z-report and locks the shift.
- **Cash in / out** of the drawer during a shift, e.g. generator fuel or change from the bank.
- **Manager PIN rules**, set by the owner and applied on every till:
  - Discount limit and refund limit.
  - Removing a scanned item, cancelling a bill, taking cash out, opening the drawer without a sale.
  - A senior cashier can be given an approver PIN.
- **Activity log** for the owner, which can't be edited or deleted:
  - Every approval, refund, cash movement, count, shift close and lockout.
  - Filters for "needs attention", approvals, money and stock.
  - Totals for the day.
- **Direct printing (desktop app):**
  - Each receipt, return slip and Z-report is drawn as an image and sent straight to the 80 mm / 58 mm thermal printer: no dialog, paper cut, and the cash drawer opens on cash sales. Because it's an image, **Urdu prints on any thermal printer**.
  - Printer setup is per till, from the Till header (owner only), with test print and test drawer buttons.
  - In a plain browser the print dialog is used instead.
- **Server messages in Urdu:** errors carry a code and values, so an Urdu-speaking cashier sees messages like "stock short" or "PIN needed" in Urdu.

### Milestone 1: foundation + Till

- **Store server** (`crates/pos-server`): the store's SQLite database and the sale rules, served to every till in the shop over the local network.
  - **Batches:** stock is kept per batch and sold earliest-expiry first. Expired stock is never sold, and each batch keeps the price printed on its pack.
  - **Selling:** medicines can be sold by Box / Strip / Tablet. Weight items (sugar etc.) can be sold by **amount (Rs 10 of sugar)** or **grams**.
  - **Money:** prices include sales tax, and bill discounts are supported. Payments can be Cash (with change), Card, JazzCash, Easypaisa or Khata (udhaar) with a credit limit.
  - **Manager PIN:** needed for a discount above 5% and for khata over the customer's limit. Approvals go into an append-only activity log.
  - **Sign-in:** cashiers use a 4-digit PIN and the owner uses a password. A user is locked out for 5 minutes after 5 wrong tries.
  - **Safe retries:** if a till's connection drops while paying, it can resend the sale and it is still only charged once.
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

Not built yet: stock count, transfers, full reports. See the roadmap below.

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

The demo store also has two suppliers, one purchase, a Rs 3,400 fuel expense and an old khata balance for Haji Rasheed.

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

`%APPDATA%\pk.medstore.pos\store.sqlite` on the main PC, with daily copies in `%APPDATA%\pk.medstore.pos\backups\`. To bring back a daily copy: close the app, replace `store.sqlite` with the copy (renamed to `store.sqlite`), and start the app.

### Cloud backup (Turso)

1. Make a free account at [turso.tech](https://turso.tech) and install the Turso CLI.
2. Make one database per store, and a token for it:
   ```bash
   turso db create store-b1
   turso db show store-b1 --url          # libsql://store-b1-yourname.turso.io
   turso db tokens create store-b1       # the token for that store's main PC
   ```
3. On that store's main PC: Backoffice › Store, Users & Settings › Cloud backup. Paste the URL and token, then **Turn on cloud backup**. The first upload sends everything.
4. For the head-office view, make a read-only token for each store (`turso db tokens create store-b2 --read-only`) and add it on the owner's PC under **All stores › Add a store**.

Keep each store's URL and token written down somewhere safe; they are what a new main PC needs to restore the store.

### Printing

1. Sign in as the owner on the till.
2. Click **Printer** in the header and pick the thermal printer (USB or network, as installed in Windows).
3. Choose 80 mm or 58 mm paper, then try **Print test receipt** and **Test cash drawer**.

The drawer must be cabled to the printer (RJ11 "DK" port); the app sends the standard pulse on pin 2. If no printer is chosen, the normal Windows print dialog is used.

## Project layout

```
crates/pos-server/   Store server: database, migrations, sale/return/shift rules, HTTP API (Rust, axum, rusqlite)
  src/cloud/         Turso client, cloud sync and restore, daily backup files, head-office view
  src/reports.rs     All financial and stock reports
  migrations/        SQL schema
  tests/             End-to-end sale, return, shift and API tests
crates/pos-printer/  ESC/POS image printing, paper cut, cash drawer; raw printing to Windows printers
src/                 Till UI (React + TypeScript)
  api/               API client and types
  domain/            Money maths and cart (mirrors the server)
  i18n/              English / Urdu text
  print/             Receipt / return slip / Z-report layout, screen preview, printer image
  screens/           Device setup, store setup, sign-in, open shift, Till, Returns, Close shift, Activity
  backoffice/        Dashboard, Inventory, Product form, Excel import, Purchases, Suppliers, Expenses, Customers & Khata, Settings
src-tauri/           Desktop shell: main-PC / extra-till mode, starts the store server
```

## Checks

```bash
npm run typecheck && npm test && npm run build
cargo test --workspace --exclude medstore-pos
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI runs all of these on every push.

## Roadmap

1. **Foundation + Till** — done.
2. **Daily control** — done:
   - Returns, close shift, cash in/out, approval rules and activity log.
   - Direct ESC/POS printing with Urdu, and the cash drawer.
   - Urdu server messages.
3. **Backoffice** — done:
   - Dashboard, inventory, add/edit product, Excel import, stock corrections.
   - Purchases (GRN) with bonus and freight, supplier payables.
   - Expenses, customers & khata with WhatsApp reminders, users, store settings, recovery code.
4. **Cloud** — done:
   - Turso backup (one database per store), restore on a new PC, daily backup files.
   - Head-office view of all stores.
5. **Reports** — done: P&L, product profit, sales by product / category / cashier / payment, cash book, stock valuation and movement, expiry, slow stock, purchases, payables, khata ageing, sales tax.
6. **Extras:**
   - Stock count and branch transfers.
   - Optional FBR POS integration (off by default, per store).
   - An extra till keeps selling if the shop network drops, then catches up.
