// Reading the owner's product sheet (Excel) into rows for the store server.
// The template's column titles are the contract; a few common alternatives
// ("Product name", "MRP"…) are accepted too.

import type { ImportRow } from '../api/types';
import { parseExpiry } from './dates';
import { parseRupees } from './money';

type Kind = 'text' | 'rupees' | 'int' | 'percent' | 'yesno' | 'expiry';

export interface Column {
  key: keyof Omit<ImportRow, 'row'>;
  title: string;
  kind: Kind;
  aliases?: string[];
  width: number;
}

export const COLUMNS: Column[] = [
  { key: 'name', title: 'Name *', kind: 'text', aliases: ['product name', 'product', 'item name', 'item'], width: 30 },
  { key: 'genericName', title: 'Generic / formula', kind: 'text', aliases: ['generic', 'formula', 'salt'], width: 22 },
  { key: 'company', title: 'Company', kind: 'text', aliases: ['manufacturer', 'brand'], width: 14 },
  { key: 'category', title: 'Category', kind: 'text', width: 16 },
  { key: 'kind', title: 'Type (Medicine / General)', kind: 'text', aliases: ['type'], width: 14 },
  { key: 'sellMode', title: 'Sold by (Pack / Weight)', kind: 'text', aliases: ['sold by'], width: 12 },
  { key: 'baseUnit', title: 'Smallest unit (Tab, Cap, Btl, Pc)', kind: 'text', aliases: ['smallest unit', 'unit'], width: 14 },
  { key: 'basePrice', title: 'Price of smallest unit (Rs)', kind: 'rupees', aliases: ['unit price', 'price'], width: 12 },
  { key: 'baseSellable', title: 'Sell smallest unit loose? (Y/N)', kind: 'yesno', aliases: ['sell loose'], width: 12 },
  { key: 'pack2Name', title: 'Pack 2 name (e.g. Strip)', kind: 'text', aliases: ['pack 2 name'], width: 12 },
  { key: 'pack2Qty', title: 'Pack 2 holds (smallest units)', kind: 'int', aliases: ['pack 2 holds'], width: 12 },
  { key: 'pack2Price', title: 'Pack 2 price (Rs)', kind: 'rupees', aliases: ['pack 2 price'], width: 12 },
  { key: 'pack3Name', title: 'Pack 3 name (e.g. Box)', kind: 'text', aliases: ['pack 3 name'], width: 12 },
  { key: 'pack3Qty', title: 'Pack 3 holds (smallest units)', kind: 'int', aliases: ['pack 3 holds'], width: 12 },
  { key: 'pack3Price', title: 'Pack 3 price (Rs)', kind: 'rupees', aliases: ['pack 3 price'], width: 12 },
  { key: 'pricePerKg', title: 'Price per kg (Rs, weight items)', kind: 'rupees', aliases: ['price per kg'], width: 12 },
  { key: 'barcode', title: 'Barcode', kind: 'text', width: 16 },
  { key: 'barcodePack', title: 'Barcode is on (pack name)', kind: 'text', aliases: ['barcode pack'], width: 12 },
  { key: 'rack', title: 'Rack', kind: 'text', aliases: ['location', 'shelf'], width: 8 },
  { key: 'taxRateBp', title: 'Sales tax %', kind: 'percent', aliases: ['tax', 'gst', 'tax %'], width: 8 },
  { key: 'rxRequired', title: 'Prescription needed? (Y/N)', kind: 'yesno', aliases: ['prescription', 'rx'], width: 10 },
  { key: 'reorderLevel', title: 'Reorder when below (smallest units)', kind: 'int', aliases: ['reorder level', 'reorder'], width: 12 },
  { key: 'openingQty', title: 'Opening stock (smallest units, or grams)', kind: 'int', aliases: ['opening stock', 'stock', 'qty'], width: 14 },
  { key: 'batchNo', title: 'Batch no', kind: 'text', aliases: ['batch'], width: 10 },
  { key: 'expiry', title: 'Expiry (MM/YYYY)', kind: 'expiry', aliases: ['expiry', 'exp'], width: 10 },
  { key: 'costPerBase', title: 'Cost (Rs) of smallest unit, or per kg', kind: 'rupees', aliases: ['cost', 'cost price'], width: 12 },
];

/** Two example rows for the template. */
export const EXAMPLES: (string | number)[][] = [
  ['Panadol 500mg Tablet', 'Paracetamol', 'GSK', 'Tablets', 'Medicine', 'Pack', 'Tab', 3, 'Y', 'Strip', 10, 30, 'Box', 200, 590, '', '8964000123456', 'Box', 'A3', 0, 'N', 200, 1000, 'B-2291', '08/2028', 2.5],
  ['Sugar', '', '', 'Grocery', 'General', 'Weight', 'g', '', '', '', '', '', '', '', '', 160, '', '', 'G4', 0, 'N', 5000, 38500, '', '', 140],
];

export type Cell = string | number | boolean | Date | null | undefined;

const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim();

function headerIndex(headers: Cell[]): Map<Column['key'], number> {
  const found = new Map<Column['key'], number>();
  const titles = headers.map((h) => (h === null || h === undefined ? '' : norm(String(h))));
  for (const col of COLUMNS) {
    const names = [col.title, ...(col.aliases ?? [])].map(norm);
    const i = titles.findIndex((t, idx) => t !== '' && names.includes(t) && ![...found.values()].includes(idx));
    if (i >= 0) found.set(col.key, i);
  }
  return found;
}

function asText(v: Cell): string | undefined {
  if (v === null || v === undefined) return undefined;
  if (v instanceof Date) return undefined;
  const s = String(v).trim();
  return s === '' ? undefined : s;
}

type Parsed = { ok: true; value: unknown } | { ok: false };

function parseCell(kind: Kind, v: Cell): Parsed {
  if (v === null || v === undefined || (typeof v === 'string' && v.trim() === '')) return { ok: true, value: undefined };
  switch (kind) {
    case 'text':
      return { ok: true, value: asText(v) };
    case 'rupees': {
      if (typeof v === 'number') return v >= 0 ? { ok: true, value: Math.round(v * 100) } : { ok: false };
      const p = parseRupees(String(v));
      return p === null ? { ok: false } : { ok: true, value: p };
    }
    case 'int': {
      const n = typeof v === 'number' ? v : Number(String(v).replace(/,/g, ''));
      // "1.5" tablets is a typing mistake to point out, not something to round away.
      return Number.isInteger(n) && n >= 0 ? { ok: true, value: n } : { ok: false };
    }
    case 'percent': {
      const n = typeof v === 'number' ? v : Number(String(v).replace('%', '').trim());
      if (!Number.isFinite(n)) return { ok: false };
      // A cell formatted as % holds 0.18 for 18%; no tax rate is below 1%.
      // Typed text ("0.5", "18%") is taken as written.
      const pct = typeof v === 'number' && v > 0 && v < 1 ? v * 100 : n;
      return pct >= 0 && pct <= 100 ? { ok: true, value: Math.round(pct * 100) } : { ok: false };
    }
    case 'yesno': {
      if (typeof v === 'boolean') return { ok: true, value: v };
      const s = String(v).trim().toLowerCase();
      if (['y', 'yes', '1', 'true', 'ہاں', 'han', 'haan'].includes(s)) return { ok: true, value: true };
      if (['n', 'no', '0', 'false', 'نہیں', 'nahi'].includes(s)) return { ok: true, value: false };
      return { ok: false };
    }
    case 'expiry': {
      if (v instanceof Date) {
        return { ok: true, value: `${v.getFullYear()}-${String(v.getMonth() + 1).padStart(2, '0')}` };
      }
      const e = parseExpiry(String(v));
      return e ? { ok: true, value: e } : { ok: false };
    }
  }
}

export interface SheetRead {
  rows: ImportRow[];
  /** Rows that couldn't be read, by sheet row number. */
  problems: { row: number; name: string; message: string }[];
  /** Template columns not found in the sheet. */
  missing: string[];
}

/**
 * Reads sheet cells (first row with a "Name" column is the header) into
 * import rows. Row numbers match what Excel shows (1-based).
 */
export function readSheet(data: Cell[][]): SheetRead {
  let headerRow = data.findIndex((r) => headerIndex(r).has('name'));
  if (headerRow < 0) headerRow = 0;
  const index = headerIndex(data[headerRow] ?? []);
  const missing = COLUMNS.filter((c) => !index.has(c.key) && ['name', 'baseUnit'].includes(c.key)).map((c) => c.title);
  const rows: ImportRow[] = [];
  const problems: SheetRead['problems'] = [];
  for (let r = headerRow + 1; r < data.length; r++) {
    const cells = data[r] ?? [];
    if (cells.every((c) => c === null || c === undefined || String(c).trim() === '')) continue;
    const row: Record<string, unknown> = { row: r + 1, name: '', kind: '', sellMode: '', baseUnit: '' };
    const bad: string[] = [];
    for (const col of COLUMNS) {
      const i = index.get(col.key);
      if (i === undefined) continue;
      const parsed = parseCell(col.kind, cells[i]);
      if (!parsed.ok) bad.push(col.title.replace(' *', ''));
      else if (parsed.value !== undefined) row[col.key] = parsed.value;
    }
    // Weight items: the cost column is per kg, the server wants per gram.
    const sellMode = String(row.sellMode ?? '').toLowerCase();
    if ((sellMode === 'weight' || sellMode === 'kg') && typeof row.costPerBase === 'number') {
      row.costPerBase = Math.round(row.costPerBase / 1000);
    }
    if (bad.length) {
      problems.push({ row: r + 1, name: String(row.name ?? ''), message: `Can't read: ${bad.join(', ')}` });
    } else {
      rows.push(row as unknown as ImportRow);
    }
  }
  return { rows, problems, missing };
}
