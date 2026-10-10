import type {
  BarcodeHit,
  Customer,
  Health,
  Language,
  LoginTile,
  Product,
  Receipt,
  SaleInput,
  Shift,
  User,
} from './types';

/** An error from the store server, with its stable code and a message for people. */
export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
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
    let data: { error?: string; message?: string } = {};
    try {
      data = await res.json();
    } catch {
      // not JSON; fall back to the status text
    }
    throw new ApiError(res.status, data.error ?? 'http_error', data.message ?? res.statusText);
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
};
