import { describe, expect, it } from 'vitest';
import { formatStock } from './stock';

const panadol = [
  { name: 'Tab', baseQty: 1 },
  { name: 'Strip', baseQty: 10 },
  { name: 'Box', baseQty: 200 },
];

describe('stock display', () => {
  it('counts in strips with loose tablets', () => {
    expect(formatStock(1240, 'unit', 'tab', panadol)).toBe('124 Strip');
    expect(formatStock(1243, 'unit', 'tab', panadol)).toBe('124 Strip + 3 Tab');
    expect(formatStock(7, 'unit', 'tab', panadol)).toBe('7 tab');
  });

  it('shows weight items in kg', () => {
    expect(formatStock(38_500, 'weight', 'g', [])).toBe('38.5 kg');
  });

  it('uses the base unit when there are no packs', () => {
    expect(formatStock(22, 'unit', 'btl', [{ name: 'Bottle', baseQty: 1 }])).toBe('22 btl');
  });
});
