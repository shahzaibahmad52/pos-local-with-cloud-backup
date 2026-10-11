import { describe, expect, it } from 'vitest';
import type { Catalogue, Product, StoreHeader, User } from '../api/types';
import { buildOfflineSale, findBarcode, localTimestamp, offlineReceiptNo, searchCatalogue } from './offline';

const product = (id: string, name: string, generic: string | null, extra: Partial<Product> = {}): Product => ({
  id,
  name,
  genericName: generic,
  company: 'GSK',
  category: null,
  kind: 'medicine',
  sellMode: 'unit',
  baseUnit: 'tab',
  pricePerKg: null,
  taxRateBp: 0,
  rxRequired: false,
  controlled: false,
  coldChain: false,
  rack: null,
  units: [
    { id: `${id}-tab`, name: 'Tab', nameUr: null, baseQty: 1, price: 300 },
    { id: `${id}-strip`, name: 'Strip', nameUr: null, baseQty: 10, price: 3_000 },
  ],
  stockBase: 100,
  nextBatchNo: 'B-1',
  nextExpiry: '2028-08',
  ...extra,
});

const panadol = product('p1', 'Panadol 500mg Tablet', 'Paracetamol');
const calpol = product('p2', 'Calpol Syrup', 'Paracetamol');
const sugar = product('p3', 'Sugar (loose)', null, { sellMode: 'weight', baseUnit: 'g', pricePerKg: 16_000, units: [], taxRateBp: 1_800 });

const store: StoreHeader = {
  code: 'B1',
  name: 'Shifa Medical',
  address: null,
  phone: null,
  ntn: null,
  drugLicense: null,
  receiptFooter: null,
  receiptFooterUr: null,
  receiptLanguage: 'en',
};

const cat: Catalogue = {
  store,
  products: [panadol, calpol, sugar],
  barcodes: [['8964000123463', 'p1', 'p1-strip']],
  maxDiscountBp: 500,
  weightRoundTo: 100,
  priceListId: 'list-1',
  lastOfflineNo: 0,
  generatedAt: '2026-10-11T09:00:00+05:00',
};

const ali: User = { id: 'u1', name: 'Ali', role: 'cashier', canApprove: false, language: 'en' };

describe('offline till', () => {
  it('numbers receipts in its own X series', () => {
    expect(offlineReceiptNo('B1', 'T02', 7)).toBe('B1-T02-X000007');
  });

  it('writes local time with its offset', () => {
    expect(localTimestamp(new Date())).toMatch(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d[+-]\d\d:\d\d$/);
  });

  it('searches the copy by name, generic name and barcode', () => {
    expect(searchCatalogue(cat.products, 'para').map((p) => p.id)).toEqual(['p1', 'p2']);
    expect(searchCatalogue(cat.products, 'calpol syr').map((p) => p.id)).toEqual(['p2']);
    expect(searchCatalogue(cat.products, '  ')).toEqual([]);
    expect(findBarcode(cat, '8964000123463')).toEqual({ product: panadol, unitId: 'p1-strip' });
    expect(findBarcode(cat, '000')).toBeNull();
  });

  it('builds the sale and its receipt with the prices the customer paid', () => {
    const sale = buildOfflineSale({
      store,
      tillCode: 'T02',
      user: ali,
      receiptNo: 'B1-T02-X000001',
      createdAt: '2026-10-11T10:15:00+05:00',
      lines: [
        { key: 'a', kind: 'unit', product: panadol, unitId: 'p1-strip', qty: 2 },
        { key: 'b', kind: 'weight', product: sugar, mode: 'amount', value: 10_000 },
      ],
      discountBp: 0,
      method: 'cash',
      tendered: 20_000,
      clientRef: 'ref-1',
      customer: null,
      weightRoundTo: 100,
      priceListId: 'list-1',
    });
    expect(sale.input.offline?.lines).toEqual([
      { unitPrice: 3_000, lineTotal: 6_000 },
      { unitPrice: 16_000, lineTotal: 10_000 },
    ]);
    expect(sale.input.lines[1]).toEqual({ productId: 'p3', amount: 10_000 });
    expect(sale.input.payments).toEqual([{ method: 'cash', amount: 20_000 }]);
    expect(sale.receipt.total).toBe(16_000);
    expect(sale.receipt.changeDue).toBe(4_000);
    expect(sale.receipt.lines[1]).toMatchObject({ sellMode: 'weight', qty: 625, unitName: 'g' });
    expect(sale.receipt.payments).toEqual([{ method: 'cash', amount: 16_000 }]);
  });
});
