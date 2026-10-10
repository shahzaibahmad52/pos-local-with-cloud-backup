import { describe, expect, it } from 'vitest';
import { ean13CheckDigit, inStoreBarcode, isValidEan13 } from './barcode';

describe('barcodes', () => {
  it('computes EAN-13 check digits', () => {
    expect(ean13CheckDigit('400638133393')).toBe(1);
    expect(isValidEan13('4006381333931')).toBe(true);
    expect(isValidEan13('4006381333932')).toBe(false);
  });

  it('makes valid in-store codes', () => {
    const code = inStoreBarcode();
    expect(code).toMatch(/^20\d{11}$/);
    expect(isValidEan13(code)).toBe(true);
  });
});
