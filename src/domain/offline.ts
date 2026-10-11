// Selling while the main PC can't be reached.
//
// An extra till keeps a copy of the product list. If the network drops it
// keeps selling from that copy, numbers its receipts in its own "X" series
// (B1-T02-X000001) and sends the sales across once the main PC answers again.

import type { Catalogue, PaymentMethod, Product, Receipt, SaleInput, StoreHeader, User } from '../api/types';
import { cartTotals, priceLine, toSaleLines, type CartLine } from './cart';

/** A sale made offline, waiting to be sent to the main PC. */
export interface QueuedSale {
  input: SaleInput;
  receipt: Receipt;
  userId: string;
}

/** A queued sale the main PC refused. Kept so the owner can see it. */
export interface RefusedSale {
  receiptNo: string;
  total: number;
  reason: string;
  at: string;
  /** The sale itself, so nothing is lost. */
  sale?: QueuedSale;
}

export const OFFLINE_METHODS: PaymentMethod[] = ['cash', 'card', 'jazzcash', 'easypaisa'];

export function offlineReceiptNo(storeCode: string, tillCode: string, n: number): string {
  return `${storeCode}-${tillCode}-X${String(n).padStart(6, '0')}`;
}

/** The local time with its offset, as the main PC writes it (2026-10-11T10:15:00+05:00). */
export function localTimestamp(d = new Date()): string {
  const pad = (n: number) => String(Math.abs(n)).padStart(2, '0');
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? '+' : '-';
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}` +
    `${sign}${pad(Math.trunc(off / 60))}:${pad(off % 60)}`
  );
}

const norm = (s: string) => s.toLowerCase().replace(/\s+/g, ' ').trim();

/** Same idea as the main PC's search: every word must match the name, generic name or company. */
export function searchCatalogue(products: Product[], query: string, limit = 20): Product[] {
  const words = norm(query).split(' ').filter(Boolean);
  if (words.length === 0) return [];
  const hits = products.filter((p) => {
    const hay = norm(`${p.name} ${p.genericName ?? ''} ${p.company ?? ''}`);
    return words.every((w) => hay.includes(w));
  });
  // Names that start with the query first.
  const first = words[0];
  hits.sort((a, b) => Number(!norm(a.name).startsWith(first)) - Number(!norm(b.name).startsWith(first)));
  return hits.slice(0, limit);
}

export function findBarcode(cat: Catalogue, code: string): { product: Product; unitId: string | null } | null {
  const hit = cat.barcodes.find((b) => b[0] === code);
  if (!hit) return null;
  const product = cat.products.find((p) => p.id === hit[1]);
  return product ? { product, unitId: hit[2] } : null;
}

/** Builds the sale and its receipt on the till itself. */
export function buildOfflineSale(args: {
  store: StoreHeader;
  tillCode: string;
  user: User;
  receiptNo: string;
  createdAt: string;
  lines: CartLine[];
  discountBp: number;
  method: PaymentMethod;
  tendered: number;
  clientRef: string;
  customer: { id: string; name: string } | null;
  weightRoundTo: number;
  priceListId: string;
}): QueuedSale {
  const totals = cartTotals(args.lines, args.discountBp, args.weightRoundTo);
  const priced = args.lines.map((l) => priceLine(l, args.weightRoundTo));
  const changeDue = args.method === 'cash' ? Math.max(0, args.tendered - totals.total) : 0;
  // Cash is recorded as what stayed in the drawer.
  const payments = [{ method: args.method, amount: totals.total }];
  const input: SaleInput = {
    tillCode: args.tillCode,
    customerId: args.customer?.id,
    lines: toSaleLines(args.lines),
    discountBp: args.discountBp,
    payments: [{ method: args.method, amount: args.method === 'cash' ? args.tendered : totals.total }],
    clientRef: args.clientRef,
    offline: {
      receiptNo: args.receiptNo,
      createdAt: args.createdAt,
      lines: priced.map((p) => ({ unitPrice: p.unitPrice, lineTotal: p.total })),
      priceListId: args.priceListId,
    },
  };
  const receipt: Receipt = {
    id: args.clientRef,
    receiptNo: args.receiptNo,
    createdAt: args.createdAt,
    tillCode: args.tillCode,
    cashierName: args.user.name,
    customerName: args.customer?.name ?? null,
    customerReceiptLanguage: null,
    lines: args.lines.map((l, i) => {
      const unit = l.kind === 'unit' ? l.product.units.find((u) => u.id === l.unitId) : undefined;
      return {
        productName: l.product.name,
        unitName: l.kind === 'unit' ? (unit?.name ?? '') : 'g',
        sellMode: l.kind === 'unit' ? 'unit' : 'weight',
        qty: l.kind === 'unit' ? l.qty : (priced[i].grams ?? 0),
        unitPrice: priced[i].unitPrice,
        lineTotal: priced[i].total,
        taxIncluded: priced[i].tax,
        batchNo: l.product.nextBatchNo,
      };
    }),
    itemCount: totals.itemCount,
    subtotal: totals.subtotal,
    discount: totals.discount,
    taxIncluded: totals.tax,
    total: totals.total,
    paid: totals.total,
    changeDue,
    payments,
    khataBalance: null,
    store: args.store,
  };
  return { input, receipt, userId: args.user.id };
}

// ---- Storage on this till ----

const KEY = {
  catalogue: 'medstore.catalogue',
  queue: 'medstore.offline.queue',
  refused: 'medstore.offline.refused',
  counter: (till: string) => `medstore.offline.no.${till}`,
};

/**
 * What this session wrote, kept in memory too. If the browser won't store
 * anything (blocked, full, a private window) the till still numbers and
 * queues offline sales correctly for as long as it stays open.
 */
const memory = new Map<string, string>();
/** Keys whose last save to the browser's storage failed. */
const unsaved = new Set<string>();

function read<T>(key: string, fallback: T): T {
  try {
    const raw = memory.get(key) ?? window.localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: unknown): void {
  const raw = JSON.stringify(value);
  memory.set(key, raw);
  try {
    window.localStorage.setItem(key, raw);
    unsaved.delete(key);
  } catch {
    unsaved.add(key);
  }
}

/** The highest X number among this till's sales still waiting to be sent. */
function highestQueued(till: string): number {
  const nos = read<QueuedSale[]>(KEY.queue, [])
    .map((q) => q.receipt.receiptNo)
    .filter((no) => no.includes(`-${till}-X`))
    .map((no) => Number(no.slice(no.lastIndexOf('-X') + 2)) || 0);
  return Math.max(0, ...nos);
}

export const offlineStore = {
  catalogue: () => read<Catalogue | null>(KEY.catalogue, null),
  saveCatalogue: (c: Catalogue) => write(KEY.catalogue, c),
  queue: () => read<QueuedSale[]>(KEY.queue, []),
  saveQueue: (q: QueuedSale[]) => write(KEY.queue, q),
  refused: () => read<RefusedSale[]>(KEY.refused, []),
  saveRefused: (r: RefusedSale[]) => write(KEY.refused, r),
  /** Takes the next X number: never one the main PC or the waiting queue already has. */
  nextNo: (till: string, serverLast: number): number => {
    const n = Math.max(read<number>(KEY.counter(till), 0), serverLast, highestQueued(till)) + 1;
    write(KEY.counter(till), n);
    return n;
  },
  /** False when offline sales are only held in memory: closing the till would lose them. */
  saved: (till: string): boolean => !unsaved.has(KEY.queue) && !unsaved.has(KEY.counter(till)),
};
