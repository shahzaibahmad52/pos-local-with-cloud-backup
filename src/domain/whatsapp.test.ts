import { describe, expect, it } from 'vitest';
import { waLink, waNumber } from './whatsapp';

describe('whatsapp links', () => {
  it('reads Pakistani mobile numbers', () => {
    expect(waNumber('0300-1234567')).toBe('923001234567');
    expect(waNumber('+92 300 1234567')).toBe('923001234567');
    expect(waNumber('3001234567')).toBe('923001234567');
    expect(waNumber('042-1234567')).toBeNull();
    expect(waNumber('0300-xxxxxxx')).toBeNull();
  });

  it('builds a link with the message', () => {
    expect(waLink('03001234567', 'Rs 5,000 due')).toBe('https://wa.me/923001234567?text=Rs%205%2C000%20due');
  });

  it('understands +92 written with the 0 kept', () => {
    expect(waNumber('+92 0300 1234567')).toBe('923001234567');
  });
});
