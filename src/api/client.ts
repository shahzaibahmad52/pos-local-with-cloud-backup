import type {
  ActivityEntry,
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
  ShiftSummary,
  TillAction,
  User,
} from './types';

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
};
