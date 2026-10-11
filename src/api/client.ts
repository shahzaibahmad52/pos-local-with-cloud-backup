import type {
  ActivityEntry,
  CloudPage,
  CountHead,
  CountView,
  RackProgress,
  TransferNote,
  TransferRow,
  TransferView,
  Report,
  StoreFigures,
  AdjustReason,
  Dashboard,
  Expense,
  ImportReport,
  ImportRow,
  InventoryPage,
  InventoryTab,
  KhataAccount,
  KhataList,
  KhataPayMethod,
  KhataPaymentSlip,
  KhataStatement,
  LastBuy,
  MoneySource,
  MonthExpenses,
  OpenTill,
  ProductDetail,
  ProductInput,
  Purchase,
  PurchaseInput,
  ReceiptLanguage,
  StoreSettings,
  Supplier,
  SupplierLedgerLine,
  UserRow,
  ApprovalInput,
  ApprovalRules,
  BarcodeHit,
  Customer,
  Health,
  Language,
  LoginTile,
  Product,
  Receipt,
  ReturnInput,
  ReturnSlip,
  SaleForReturn,
  SaleInput,
  Shift,
  StoreHeader,
  ShiftSummary,
  TillAction,
  User,
} from './types';

const q = (params: Record<string, string | number | undefined>) =>
  Object.entries(params)
    .filter(([, v]) => v !== undefined && v !== '')
    .map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
    .join('&');
const id = encodeURIComponent;

/** An error from the store server, with its stable code and a message for people. */
export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
    /** Why, in a form the till can translate (e.g. `stock.short`). */
    public reason: string | null = null,
    /** Values the message was built from. */
    public params: Record<string, unknown> = {},
  ) {
    super(message);
  }
}

let baseUrl = '';
let token: string | null = null;

export function configureApi(url: string): void {
  baseUrl = url.replace(/\/$/, '');
}

export function setToken(value: string | null): void {
  token = value;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(baseUrl + path, {
      method,
      headers: {
        ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, 'network', "Can't reach the main PC. Check the network.");
  }
  if (!res.ok) {
    let data: { error?: string; message?: string; reason?: string; params?: Record<string, unknown> } = {};
    try {
      data = await res.json();
    } catch {
      // not JSON; fall back to the status text
    }
    throw new ApiError(res.status, data.error ?? 'http_error', data.message ?? res.statusText, data.reason ?? null, data.params ?? {});
  }
  return (await res.json()) as T;
}

export const api = {
  health: () => request<Health>('GET', '/api/health'),
  setup: (input: {
    storeCode: string;
    storeName: string;
    address?: string;
    phone?: string;
    ownerName: string;
    ownerPassword: string;
  }) => request<{ token: string; user: User }>('POST', '/api/setup', input),
  loginTiles: () => request<LoginTile[]>('GET', '/api/login-tiles'),
  login: (userId: string, secret: string) =>
    request<{ token: string; user: User }>('POST', '/api/login', { userId, secret }),
  logout: () => request<{ ok: boolean }>('POST', '/api/logout'),
  setLanguage: (language: Language) => request<{ ok: boolean }>('PATCH', '/api/me/language', { language }),
  searchProducts: (q: string) => request<Product[]>('GET', `/api/products/search?q=${encodeURIComponent(q)}`),
  byBarcode: (code: string) => request<BarcodeHit>('GET', `/api/products/barcode/${encodeURIComponent(code)}`),
  currentShift: (tillCode: string) =>
    request<Shift | null>('GET', `/api/shifts/current?tillCode=${encodeURIComponent(tillCode)}`),
  openShift: (tillCode: string, openingCash: number) =>
    request<Shift>('POST', '/api/shifts/open', { tillCode, openingCash }),
  createSale: (input: SaleInput) => request<Receipt>('POST', '/api/sales', input),
  receipt: (id: string) => request<Receipt>('GET', `/api/sales/${encodeURIComponent(id)}/receipt`),
  customers: (q = '') => request<Customer[]>('GET', `/api/customers?q=${encodeURIComponent(q)}`),
  createCustomer: (input: { name: string; phone?: string }) => request<Customer>('POST', '/api/customers', input),
  rules: () => request<ApprovalRules>('GET', '/api/rules'),
  saveRules: (rules: ApprovalRules) => request<ApprovalRules>('PUT', '/api/rules', rules),
  recordAction: (input: { action: TillAction; tillCode: string; details?: unknown; approval?: ApprovalInput }) =>
    request<{ ok: boolean; approvedBy: string | null }>('POST', '/api/actions', input),
  activity: (date: string, filter = 'all') =>
    request<ActivityEntry[]>('GET', `/api/activity?date=${encodeURIComponent(date)}&filter=${encodeURIComponent(filter)}`),
  saleForReturn: (receiptNo: string) =>
    request<SaleForReturn>('GET', `/api/returns/sale/${encodeURIComponent(receiptNo.trim())}`),
  createReturn: (input: ReturnInput) => request<ReturnSlip>('POST', '/api/returns', input),
  shiftSummary: (id: string) => request<ShiftSummary>('GET', `/api/shifts/${encodeURIComponent(id)}/summary`),
  countCash: (id: string, denominations: Record<string, number>) =>
    request<ShiftSummary>('POST', `/api/shifts/${encodeURIComponent(id)}/count`, { denominations }),
  closeShift: (id: string, note: string | null) =>
    request<ShiftSummary>('POST', `/api/shifts/${encodeURIComponent(id)}/close`, { note }),
  cashMove: (input: { tillCode: string; kind: 'in' | 'out'; amount: number; reason: string; approval?: ApprovalInput }) =>
    request<ShiftSummary>('POST', '/api/cash-moves', input),

  // Backoffice
  recover: (input: { userId: string; code: string; newPassword: string }) =>
    request<{ token: string; user: User }>('POST', '/api/recover', input),
  dashboard: (from: string, to: string) => request<Dashboard>('GET', `/api/dashboard?${q({ from, to })}`),
  openTills: () => request<OpenTill[]>('GET', '/api/tills/open'),
  inventory: (p: { q?: string; tab?: InventoryTab; category?: string; offset?: number; limit?: number }) =>
    request<InventoryPage>('GET', `/api/inventory?${q(p)}`),
  productDetail: (productId: string) => request<ProductDetail>('GET', `/api/inventory/${id(productId)}`),
  createProduct: (input: ProductInput) => request<Product>('POST', '/api/products', input),
  updateProduct: (productId: string, input: ProductInput) =>
    request<ProductDetail>('PUT', `/api/products/${id(productId)}`, input),
  receiveStock: (input: { productId: string; batchNo: string; expiry?: string; baseQty: number; costPerBase: number; reason: 'opening' }) =>
    request<Product>('POST', '/api/stock/receive', input),
  adjustStock: (input: { batchId: string; newQty: number; reason: AdjustReason; note?: string }) =>
    request<ProductDetail>('POST', '/api/stock/adjust', input),
  importProducts: (rows: ImportRow[], commit: boolean) =>
    request<ImportReport>('POST', '/api/products/import', { rows, commit }),
  suppliers: () => request<Supplier[]>('GET', '/api/suppliers'),
  saveSupplier: (supplierId: string | null, input: { name: string; phone?: string; notes?: string; termsDays: number }) =>
    supplierId
      ? request<Supplier>('PUT', `/api/suppliers/${id(supplierId)}`, input)
      : request<Supplier>('POST', '/api/suppliers', input),
  supplierLedger: (supplierId: string) => request<SupplierLedgerLine[]>('GET', `/api/suppliers/${id(supplierId)}/ledger`),
  paySupplier: (supplierId: string, input: { amount: number; paidFrom: MoneySource; tillCode?: string; note?: string }) =>
    request<Supplier>('POST', `/api/suppliers/${id(supplierId)}/payments`, input),
  supplierOpening: (supplierId: string, amount: number, note?: string) =>
    request<Supplier>('POST', `/api/suppliers/${id(supplierId)}/opening`, { amount, note }),
  purchases: (supplierId?: string) => request<Purchase[]>('GET', `/api/purchases?${q({ supplierId })}`),
  purchase: (purchaseId: string) => request<Purchase>('GET', `/api/purchases/${id(purchaseId)}`),
  receivePurchase: (input: PurchaseInput) => request<Purchase>('POST', '/api/purchases', input),
  lastBuy: (productId: string) => request<LastBuy | null>('GET', `/api/purchases/last-buy/${id(productId)}`),
  expenses: (month: string) => request<MonthExpenses>('GET', `/api/expenses?${q({ month })}`),
  addExpense: (input: { category: string; amount: number; date: string; paidFrom: MoneySource; tillCode?: string; note?: string }) =>
    request<Expense>('POST', '/api/expenses', input),
  deleteExpense: (expenseId: string) => request<{ ok: boolean }>('DELETE', `/api/expenses/${id(expenseId)}`),
  khata: (p: { q?: string; tab?: 'all' | 'balance' | 'overdue' }) => request<KhataList>('GET', `/api/khata?${q(p)}`),
  khataStatement: (customerId: string) => request<KhataStatement>('GET', `/api/khata/${id(customerId)}`),
  khataPayment: (customerId: string, input: { amount: number; method: KhataPayMethod; tillCode?: string; note?: string }) =>
    request<KhataPaymentSlip>('POST', `/api/khata/${id(customerId)}/payments`, input),
  khataOpening: (customerId: string, amount: number, note?: string) =>
    request<KhataAccount>('POST', `/api/khata/${id(customerId)}/opening`, { amount, note }),
  updateCustomer: (
    customerId: string,
    input: { name: string; phone?: string; notes?: string; creditLimit: number; receiptLanguage: ReceiptLanguage | null; active: boolean },
  ) => request<KhataAccount>('PUT', `/api/customers/${id(customerId)}`, input),
  createCustomerFull: (input: { name: string; phone?: string; creditLimit: number; receiptLanguage: ReceiptLanguage | null }) =>
    request<Customer>('POST', '/api/customers', input),
  settings: () => request<StoreSettings>('GET', '/api/settings'),
  updateStore: (input: {
    name: string;
    address?: string;
    phone?: string;
    ntn?: string;
    drugLicense?: string;
    receiptFooter?: string;
    receiptFooterUr?: string;
    receiptLanguage: ReceiptLanguage;
  }) => request<StoreHeader>('PUT', '/api/store', input),
  users: () => request<UserRow[]>('GET', '/api/users'),
  createUser: (input: { name: string; role: 'admin' | 'cashier'; secret: string; canApprove: boolean; language: Language }) =>
    request<User>('POST', '/api/users', input),
  updateUser: (userId: string, input: { name: string; canApprove: boolean; language: Language; active: boolean; secret?: string }) =>
    request<UserRow>('PUT', `/api/users/${id(userId)}`, input),
  newRecoveryCode: () => request<{ code: string }>('POST', '/api/recovery-code'),

  // Cloud backup and head office
  cloud: () => request<CloudPage>('GET', '/api/cloud'),
  enableCloud: (url: string, token: string) => request<CloudPage>('PUT', '/api/cloud', { url, token }),
  disableCloud: () => request<CloudPage>('DELETE', '/api/cloud'),
  syncNow: () => request<CloudPage>('POST', '/api/cloud/sync'),
  backupNow: () => request<CloudPage>('POST', '/api/backups'),
  restore: (url: string, token: string) => request<{ ok: boolean; storeName: string }>('POST', '/api/restore', { url, token }),
  hq: (from: string, to: string) => request<StoreFigures[]>('GET', `/api/hq?${q({ from, to })}`),
  addHqStore: (input: { url: string; token: string; name?: string }) => request<{ ok: boolean }>('POST', '/api/hq/stores', input),
  report: (name: string, from: string, to: string, n?: number) =>
    request<Report>('GET', `/api/reports/${id(name)}?${q({ from, to, n })}`),
  counts: () => request<{ counts: CountHead[]; racks: RackProgress[] }>('GET', '/api/counts'),
  startCount: (input: { section?: string; noRack?: boolean; blind: boolean }) => request<CountView>('POST', '/api/counts', input),
  count: (countId: string) => request<CountView>('GET', `/api/counts/${id(countId)}`),
  setCountLine: (countId: string, lineId: string, counted: number | null, reason?: string) =>
    request<CountView>('PUT', `/api/counts/${id(countId)}/lines/${id(lineId)}`, { counted, reason }),
  addCountProduct: (countId: string, productId: string) => request<CountView>('POST', `/api/counts/${id(countId)}/products`, { productId }),
  applyCount: (countId: string) => request<CountView>('POST', `/api/counts/${id(countId)}/apply`),
  cancelCount: (countId: string) => request<CountView>('POST', `/api/counts/${id(countId)}/cancel`),
  transfers: () => request<{ transfers: TransferRow[]; stores: { id: string; code: string; name: string }[] }>('GET', '/api/transfers'),
  transfer: (transferId: string) => request<TransferView>('GET', `/api/transfers/${id(transferId)}`),
  sendTransfer: (input: { toCode: string; toName?: string; note?: string; lines: { batchId: string; qty: number }[] }) =>
    request<TransferView>('POST', '/api/transfers', input),
  incomingTransfers: () => request<{ notes: TransferNote[]; errors: [string, string][] }>('GET', '/api/transfers/incoming'),
  receiveTransfer: (note: TransferNote, received: { qty: number; reason?: string }[]) =>
    request<TransferView>('POST', '/api/transfers/receive', { note, received }),
  removeHqStore: (storeId: string) => request<{ ok: boolean }>('DELETE', `/api/hq/stores/${id(storeId)}`),
};
