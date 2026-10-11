import { describe, expect, it } from 'vitest';
import { COLUMNS, EXAMPLES, readSheet } from './importSheet';

describe('product sheet', () => {
  it('reads the template examples', () => {
    const { rows, problems, missing } = readSheet([COLUMNS.map((c) => c.title), ...EXAMPLES]);
    expect(problems).toEqual([]);
    expect(missing).toEqual([]);
    expect(rows[0]).toMatchObject({
      row: 2,
      name: 'Panadol 500mg Tablet',
      basePrice: 300,
      baseSellable: true,
      pack2Name: 'Strip',
      pack2Qty: 10,
      pack2Price: 3_000,
      pack3Price: 59_000,
      barcode: '8964000123456',
      taxRateBp: 0,
      rxRequired: false,
      openingQty: 1000,
      expiry: '2028-08',
      costPerBase: 250,
    });
    // Sugar's cost is per kg in the sheet and per gram on the server.
    expect(rows[1]).toMatchObject({ name: 'Sugar', sellMode: 'Weight', pricePerKg: 16_000, costPerBase: 14 });
  });

  it('accepts other column names and skips blank rows', () => {
    const { rows } = readSheet([
      ['Shop price list'],
      ['Product name', 'Unit', 'Price', 'GST', 'Exp', 'Barcode'],
      ['Calpol Syrup', 'Btl', '180', 0.18, new Date(2028, 2, 1), 8964999000011],
      [null, '', null],
    ]);
    expect(rows).toEqual([
      { row: 3, name: 'Calpol Syrup', kind: '', sellMode: '', baseUnit: 'Btl', basePrice: 18_000, taxRateBp: 1_800, expiry: '2028-03', barcode: '8964999000011' },
    ]);
  });

  it('reports cells it cannot read', () => {
    const { rows, problems } = readSheet([
      ['Name *', 'Smallest unit (Tab, Cap, Btl, Pc)', 'Pack 2 holds (smallest units)', 'Expiry (MM/YYYY)'],
      ['Risek', 'Cap', 'fourteen', '13/2028'],
    ]);
    expect(rows).toEqual([]);
    expect(problems).toEqual([{ row: 2, name: 'Risek', message: "Can't read: Pack 2 holds (smallest units), Expiry (MM/YYYY)" }]);
  });

  it('flags a fractional stock count and an impossible tax rate', () => {
    const { rows, problems } = readSheet([
      ['Product name', 'Unit', 'Price', 'GST', 'Stock'],
      ['Comb', 'Pc', '50', '-5', '3'],
      ['Mask', 'Pc', '20', '0.5', '1.5'],
      ['Soap', 'Pc', '120', '18%', '6'],
    ]);
    expect(problems.map((p) => p.row)).toEqual([2, 3]);
    expect(rows.find((r) => r.name === 'Soap')).toMatchObject({ taxRateBp: 1_800, openingQty: 6 });
    // Typed text is taken as written: "0.5" is half a percent, not 50%.
    const { rows: typed } = readSheet([['Product name', 'Unit', 'Price', 'GST'], ['Mask', 'Pc', '20', '0.5']]);
    expect(typed[0].taxRateBp).toBe(50);
  });
});
