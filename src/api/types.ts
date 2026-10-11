// Shapes returned by the store server (crates/pos-server). Money is paisa.

export type Language = 'en' | 'ur';
export type ReceiptLanguage = 'en' | 'ur' | 'both';

export interface User {
  id: string;
  name: string;
  role: 'admin' | 'cashier';
  canApprove: boolean;
  language: Language;
}

export interface LoginTile {
  id: string;
  name: string;
  role: 'admin' | 'cashier';
  language: Language;
  canApprove: boolean;
}

export interface Unit {
  id: string;
  name: string;
  nameUr: string | null;
  baseQty: number;
  price: number;
}

export interface Product {
  id: string;
  name: string;
  genericName: string | null;
  company: string | null;
  category: string | null;
  kind: 'medicine' | 'general';
  sellMode: 'unit' | 'weight';
  baseUnit: string;
  pricePerKg: number | null;
  taxRateBp: number;
  rxRequired: boolean;
  controlled: boolean;
  coldChain: boolean;
  rack: string | null;
  units: Unit[];
  stockBase: number;
  nextBatchNo: string | null;
  nextExpiry: string | null;
}

export interface BarcodeHit {
  product: Product;
  unitId: string | null;
}

export interface StoreHeader {
  code: string;
  name: string;
  address: string | null;
  phone: string | null;
  ntn: string | null;
  drugLicense: string | null;
  receiptFooter: string | null;
  receiptFooterUr: string | null;
  receiptLanguage: ReceiptLanguage;
}

export interface Health {
  ok: boolean;
  setUp: boolean;
  store: StoreHeader | null;
  version: string;
}

export interface Shift {
  id: string;
  tillCode: string;
  userId: string;
  userName: string;
  openedAt: string;
  openingCash: number;
}

export interface Customer {
  id: string;
  name: string;
  phone: string | null;
  creditLimit: number;
  balance: number;
  receiptLanguage: ReceiptLanguage | null;
}

export type PaymentMethod = 'cash' | 'card' | 'jazzcash' | 'easypaisa' | 'khata';

export interface SaleLineInput {
  productId: string;
  unitId?: string;
  qty?: number;
  grams?: number;
  amount?: number;
}

export interface SaleInput {
  tillCode: string;
  customerId?: string;
  lines: SaleLineInput[];
  discountBp: number;
  payments: { method: PaymentMethod; amount: number }[];
  clientRef: string;
  approval?: ApprovalInput;
  /** Set by a till that made the sale while the main PC couldn't be reached. */
  offline?: {
    receiptNo: string;
    createdAt: string;
    lines: { unitPrice: number; lineTotal: number }[];
  };
}

/** The product list an extra till keeps, to keep selling if the network drops. */
export interface Catalogue {
  store: StoreHeader;
  products: Product[];
  /** [barcode, product id, pack id] */
  barcodes: [string, string, string | null][];
  maxDiscountBp: number;
  weightRoundTo: number;
  lastOfflineNo: number;
  generatedAt: string;
}

export interface ReceiptLine {
  productName: string;
  unitName: string;
  sellMode: 'unit' | 'weight';
  qty: number;
  unitPrice: number;
  lineTotal: number;
  taxIncluded: number;
  batchNo: string | null;
}

export interface Receipt {
  id: string;
  receiptNo: string;
  createdAt: string;
  tillCode: string;
  cashierName: string;
  customerName: string | null;
  customerReceiptLanguage: ReceiptLanguage | null;
  lines: ReceiptLine[];
  itemCount: number;
  subtotal: number;
  discount: number;
  taxIncluded: number;
  total: number;
  paid: number;
  changeDue: number;
  payments: { method: PaymentMethod; amount: number }[];
  khataBalance: number | null;
  store: StoreHeader;
}

export interface ApprovalInput {
  userId: string;
  secret: string;
}

export interface ApprovalRules {
  cashierMaxDiscountBp: number;
  refundApprovalAbove: number;
  removeItem: boolean;
  cancelBill: boolean;
  cashOut: boolean;
  noSaleDrawer: boolean;
}

export type TillAction = 'remove_item' | 'cancel_bill' | 'no_sale_drawer';

export interface ActivityEntry {
  id: string;
  createdAt: string;
  tillCode: string | null;
  userName: string | null;
  action: string;
  details: Record<string, unknown> | null;
  approvedByName: string | null;
  flagged: boolean;
}

export interface ReturnableLine {
  saleLineId: string;
  productName: string;
  unitName: string;
  sellMode: 'unit' | 'weight';
  qty: number;
  returnedQty: number;
  unitPrice: number;
  lineTotal: number;
  batchNo: string | null;
  batchExpiry: string | null;
  blocked: string | null;
}

export interface SaleForReturn {
  id: string;
  receiptNo: string;
  createdAt: string;
  cashierName: string;
  customerId: string | null;
  customerName: string | null;
  subtotal: number;
  total: number;
  refundedSoFar: number;
  paymentMethods: PaymentMethod[];
  lines: ReturnableLine[];
}

export type ReturnCondition = 'restock' | 'damaged' | 'expired';
export type RefundMethod = 'cash' | 'khata' | 'card' | 'jazzcash' | 'easypaisa';

export interface ReturnInput {
  saleId: string;
  tillCode: string;
  lines: { saleLineId: string; qty: number; condition: ReturnCondition; reason?: string }[];
  refundMethod: RefundMethod;
  clientRef: string;
  approval?: ApprovalInput;
}

export interface ReturnSlip {
  id: string;
  returnNo: string;
  saleReceiptNo: string;
  createdAt: string;
  tillCode: string;
  cashierName: string;
  customerName: string | null;
  lines: { productName: string; unitName: string; sellMode: 'unit' | 'weight'; qty: number; refund: number; condition: ReturnCondition }[];
  refundTotal: number;
  refundMethod: RefundMethod;
  approvedByName: string | null;
  khataBalance: number | null;
  store: StoreHeader;
}

export interface ShiftSummary {
  id: string;
  tillCode: string;
  cashierName: string;
  openedAt: string;
  closedAt: string | null;
  status: 'open' | 'closed';
  openingCash: number;
  billCount: number;
  salesTotal: number;
  cashSales: number;
  cashRefunds: number;
  refundsTotal: number;
  returnsCount: number;
  cashIn: number;
  cashOut: number;
  otherPayments: Record<string, number>;
  expectedCash: number | null;
  countedCash: number | null;
  difference: number | null;
  countAttempts: number;
  note: string | null;
  store: StoreHeader;
}

// ---- Backoffice (admin only) ----

/** Where money for a purchase, expense or supplier payment came from. */
export type MoneySource = 'drawer' | 'owner' | 'bank' | 'wallet';

export interface OpenTill {
  tillCode: string;
  cashierName: string;
}

export interface DashboardFigures {
  sales: number;
  bills: number;
  refunds: number;
  netSales: number;
  salesTax: number;
  costOfSales: number;
  grossProfit: number;
  expenses: number;
  netProfit: number;
}

export interface UnitPrice {
  id: string;
  name: string;
  baseQty: number;
  price: number;
}

export interface InventoryRow {
  id: string;
  name: string;
  genericName: string | null;
  company: string | null;
  category: string | null;
  kind: 'medicine' | 'general';
  sellMode: 'unit' | 'weight';
  baseUnit: string;
  pricePerKg: number | null;
  rack: string | null;
  active: boolean;
  reorderLevel: number;
  stockBase: number;
  expiredBase: number;
  stockValue: number;
  avgCost: number;
  nextExpiry: string | null;
  low: boolean;
  expiring: boolean;
  units: UnitPrice[];
}

export interface ExpiringBatch {
  productId: string;
  productName: string;
  batchNo: string;
  expiry: string;
  qtyBase: number;
  baseUnit: string;
  expired: boolean;
}

export interface Dashboard {
  from: string;
  to: string;
  figures: DashboardFigures;
  payments: [string, number][];
  khataDue: number;
  khataCustomers: number;
  payables: number;
  payablesOverdue: number;
  days: { date: string; netSales: number }[];
  lowStock: InventoryRow[];
  expiring: ExpiringBatch[];
}

export type InventoryTab = 'all' | 'low' | 'expiring' | 'expired' | 'medicine' | 'general' | 'archived';

export interface InventoryPage {
  summary: { productCount: number; stockValue: number; lowCount: number; expiringCount: number; expiredCount: number };
  rows: InventoryRow[];
  total: number;
  categories: string[];
}

export interface UnitDetail {
  id: string;
  name: string;
  nameUr: string | null;
  baseQty: number;
  price: number;
  sellable: boolean;
}

export interface BatchDetail {
  id: string;
  batchNo: string;
  expiry: string | null;
  costPerBase: number;
  qtyOnHand: number;
  receivedAt: string;
  expired: boolean;
  prices: [string, number][];
}

export interface LastBuy {
  unitName: string;
  tradePrice: number;
  discBp: number;
  salePrice: number | null;
  supplierName: string;
  invoiceDate: string;
}

export interface ProductDetail extends InventoryRow {
  taxRateBp: number;
  rxRequired: boolean;
  controlled: boolean;
  coldChain: boolean;
  allUnits: UnitDetail[];
  barcodes: { code: string; unitName: string | null }[];
  batches: BatchDetail[];
  lastBuy: LastBuy | null;
  moves: { createdAt: string; batchNo: string | null; qtyDelta: number; reason: string; userName: string | null }[];
}

export interface UnitInput {
  id?: string;
  name: string;
  nameUr?: string | null;
  baseQty: number;
  price: number;
  sellable?: boolean;
}

export interface ProductInput {
  name: string;
  genericName?: string | null;
  company?: string | null;
  category?: string | null;
  kind: 'medicine' | 'general';
  sellMode?: 'unit' | 'weight';
  baseUnit: string;
  pricePerKg?: number | null;
  taxRateBp: number;
  rxRequired: boolean;
  controlled: boolean;
  coldChain: boolean;
  rack?: string | null;
  reorderLevel: number;
  units: UnitInput[];
  barcodes: { code: string; unitName?: string | null }[];
  active?: boolean;
}

export type AdjustReason = 'damaged' | 'expired' | 'lost' | 'count' | 'other';

export interface ImportRow {
  row: number;
  name: string;
  genericName?: string;
  company?: string;
  category?: string;
  kind: string;
  sellMode: string;
  baseUnit: string;
  basePrice?: number;
  baseSellable?: boolean;
  pack2Name?: string;
  pack2Qty?: number;
  pack2Price?: number;
  pack3Name?: string;
  pack3Qty?: number;
  pack3Price?: number;
  pricePerKg?: number;
  barcode?: string;
  barcodePack?: string;
  rack?: string;
  taxRateBp?: number;
  rxRequired?: boolean;
  reorderLevel?: number;
  openingQty?: number;
  batchNo?: string;
  expiry?: string;
  costPerBase?: number;
}

export interface ImportReport {
  committed: boolean;
  newCount: number;
  updateCount: number;
  errorCount: number;
  rows: { row: number; name: string; status: 'new' | 'update' | 'error'; message: string | null }[];
}

export interface Supplier {
  id: string;
  name: string;
  phone: string | null;
  notes: string | null;
  termsDays: number;
  balance: number;
  nextDue: string | null;
  overdue: boolean;
}

export interface PurchaseLineInput {
  productId: string;
  unitId?: string;
  batchNo: string;
  expiry?: string;
  qty: number;
  bonus: number;
  tradePrice: number;
  discBp: number;
  salePrice?: number;
}

export interface PurchaseInput {
  supplierId: string;
  invoiceNo?: string;
  invoiceDate: string;
  termsDays: number;
  freight: number;
  paidNow: number;
  paidFrom: MoneySource;
  tillCode?: string;
  lines: PurchaseLineInput[];
  clientRef: string;
}

export interface Purchase {
  id: string;
  purchaseNo: string;
  supplierId: string;
  supplierName: string;
  invoiceNo: string | null;
  invoiceDate: string;
  dueDate: string | null;
  gross: number;
  discount: number;
  freight: number;
  net: number;
  paidNow: number;
  userName: string;
  createdAt: string;
  lineCount: number;
  lines: {
    productId: string;
    productName: string;
    unitName: string;
    batchNo: string;
    expiry: string | null;
    qty: number;
    bonus: number;
    tradePrice: number;
    discBp: number;
    salePrice: number | null;
    lineTotal: number;
    costPerBase: number;
  }[];
}

export interface SupplierLedgerLine {
  createdAt: string;
  detail: string;
  purchaseId: string | null;
  paidFrom: MoneySource | null;
  amount: number;
  balance: number;
}

export interface Expense {
  id: string;
  category: string;
  amount: number;
  date: string;
  paidFrom: MoneySource;
  tillCode: string | null;
  note: string | null;
  userName: string;
  createdAt: string;
  locked: boolean;
}

export interface MonthExpenses {
  month: string;
  total: number;
  byCategory: [string, number][];
  entries: Expense[];
}

export interface KhataAccount {
  id: string;
  name: string;
  phone: string | null;
  notes: string | null;
  creditLimit: number;
  receiptLanguage: ReceiptLanguage | null;
  active: boolean;
  balance: number;
  lastPaid: string | null;
  ageDays: number;
  overdue: boolean;
}

export interface KhataList {
  summary: {
    totalDue: number;
    owingCount: number;
    overdueTotal: number;
    overdueCount: number;
    collectedThisMonth: number;
    nearLimitCount: number;
  };
  accounts: KhataAccount[];
}

export type KhataPayMethod = 'cash' | 'jazzcash' | 'easypaisa' | 'bank';

export interface KhataLedgerLine {
  createdAt: string;
  kind: 'sale' | 'return' | 'opening' | KhataPayMethod;
  reference: string | null;
  note: string | null;
  amount: number;
  balance: number;
}

export interface KhataStatement {
  account: KhataAccount;
  lines: KhataLedgerLine[];
  store: StoreHeader;
}

export interface KhataPaymentSlip {
  customerName: string;
  amount: number;
  method: KhataPayMethod;
  balanceBefore: number;
  balanceAfter: number;
  createdAt: string;
  receivedBy: string;
  store: StoreHeader;
}

export interface UserRow {
  id: string;
  name: string;
  role: 'admin' | 'cashier';
  canApprove: boolean;
  language: Language;
  active: boolean;
  createdAt: string;
}

export interface StoreSettings {
  store: StoreHeader;
  hasRecoveryCode: boolean;
  version: string;
}

// ---- Cloud backup and head office ----

export interface CloudPage {
  enabled: boolean;
  url: string | null;
  pending: number;
  lastSyncAt: string | null;
  lastError: string | null;
  syncing: boolean;
  backups: { name: string; size: number }[];
  backupDir: string | null;
}

export interface StoreFigures {
  id: string | null;
  code: string;
  name: string;
  thisStore: boolean;
  figures: DashboardFigures | null;
  khataDue: number;
  lastSyncAt: string | null;
  error: string | null;
}

// ---- Reports ----

export interface ReportColumn {
  key: string;
  kind: 'text' | 'money' | 'int' | 'pct' | 'date' | 'month' | 'datetime';
}

export interface ReportLine {
  key: string;
  label: string | null;
  amount: number;
  style: 'heading' | 'line' | 'item' | 'less' | 'sub' | 'total';
  pct: number | null;
}

export interface Report {
  name: string;
  from: string;
  to: string;
  columns: ReportColumn[];
  rows: Record<string, string | number | null>[];
  totals: Record<string, number> | null;
  lines: ReportLine[];
}

// ---- Stock counts and transfers ----

export interface RackProgress {
  rack: string | null;
  lines: number;
  counted: number;
}

export interface CountHead {
  id: string;
  countNo: string;
  section: string | null;
  blind: boolean;
  status: 'open' | 'applied' | 'cancelled';
  startedBy: string;
  startedAt: string;
  finishedAt: string | null;
  lines: number;
  counted: number;
}

export interface CountLine {
  id: string;
  productId: string;
  productName: string;
  rack: string | null;
  sellMode: 'unit' | 'weight';
  baseUnit: string;
  units: UnitPrice[];
  batchNo: string;
  expiry: string | null;
  systemQty: number | null;
  countedQty: number | null;
  difference: number | null;
  costPerBase: number;
  reason: string | null;
}

export interface CountView extends CountHead {
  items: CountLine[];
  shortValue: number;
  extraValue: number;
  racks: RackProgress[];
}

export interface TransferNoteLine {
  productName: string;
  sellMode: 'unit' | 'weight';
  baseUnit: string;
  units: { name: string; nameUr: string | null; baseQty: number; price: number }[];
  batchNo: string;
  expiry: string | null;
  qty: number;
  coldChain: boolean;
}

export interface TransferNote {
  kind: string;
  transferId: string;
  transferNo: string;
  fromCode: string;
  fromName: string;
  toCode: string;
  sentAt: string;
  sentBy: string;
  note: string | null;
  lines: TransferNoteLine[];
}

export interface TransferRow {
  id: string;
  transferNo: string;
  direction: 'out' | 'in';
  otherCode: string;
  otherName: string | null;
  status: 'sent' | 'received';
  createdAt: string;
  lines: number;
  differences: number;
}

export interface TransferView {
  id: string;
  transferNo: string;
  direction: 'out' | 'in';
  otherCode: string;
  otherName: string | null;
  status: 'sent' | 'received';
  note: string | null;
  userName: string;
  createdAt: string;
  lines: { productName: string; batchNo: string; expiry: string | null; sentQty: number; receivedQty: number | null; baseUnit: string; reason: string | null }[];
  payload: TransferNote | null;
}
