import { describe, expect, it } from 'vitest';
import type { Product } from '../api/types';
import { addToCart, baseQtyOnBill, cartTotals, priceLine, toSaleLines, type CartLine } from './cart';

const panadol: Product = {
  id: 'p1',
  name: 'Panadol 500mg Tablet',
  genericName: 'Paracetamol',
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
  rack: 'A3',
  units: [
    { id: 'tab', name: 'Tab', nameUr: null, baseQty: 1, price: 300 },
    { id: 'strip', name: 'Strip', nameUr: null, baseQty: 10, price: 3_000 },
    { id: 'box', name: 'Box', nameUr: null, baseQty: 200, price: 59_000 },
  ],
  stockBase: 1_240,
  nextBatchNo: 'B-2291',
  nextExpiry: '2028-08',
};

const sugar: Product = {
  ...panadol,
  id: 's1',
  name: 'Sugar',
  kind: 'general',
  sellMode: 'weight',
  baseUnit: 'g',
  pricePerKg: 16_000,
  units: [],
  stockBase: 38_500,
};

const milk: Product = {
  ...panadol,
  id: 'm1',
  name: "Olper's Milk 1L",
  kind: 'general',
  taxRateBp: 1_800,
  units: [{ id: 'pack', name: 'Pack', nameUr: null, baseQty: 1, price: 33_000 }],
};

describe('cart', () => {
  it('adds a strip by default and counts repeats', () => {
    let lines: CartLine[] = [];
    lines = addToCart(lines, panadol);
    lines = addToCart(lines, panadol);
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatchObject({ unitId: 'strip', qty: 2 });
    lines = addToCart(lines, panadol, 'box');
    expect(lines).toHaveLength(2);
    expect(baseQtyOnBill(lines, 'p1')).toBe(220);
  });

  it('adds weight items once, starting at 1 kg', () => {
    let lines = addToCart([], sugar);
    lines = addToCart(lines, sugar);
    expect(lines).toHaveLength(1);
    expect(priceLine(lines[0]).total).toBe(16_000);
  });

  it('prices sugar asked for by amount', () => {
    const line: CartLine = { key: 'k', kind: 'weight', product: sugar, mode: 'amount', value: 1_000 };
    expect(priceLine(line)).toMatchObject({ total: 1_000, grams: 63 });
    expect(toSaleLines([line])).toEqual([{ productId: 's1', amount: 1_000 }]);
  });

  it('totals with discount and included tax like the server', () => {
    const lines: CartLine[] = [
      { key: 'a', kind: 'unit', product: panadol, unitId: 'strip', qty: 2 },
      { key: 'b', kind: 'unit', product: milk, unitId: 'pack', qty: 2 },
      { key: 'c', kind: 'weight', product: sugar, mode: 'amount', value: 10_000 },
    ];
    const t = cartTotals(lines, 0);
    expect(t).toMatchObject({ subtotal: 82_000, total: 82_000, tax: 10_068, itemCount: 5 });
    const discounted = cartTotals(lines, 500);
    expect(discounted.discount).toBe(4_100);
    expect(discounted.total).toBe(77_900);
  });
});
