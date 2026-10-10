import { describe, expect, it } from 'vitest';
import { ApiError } from '../api/client';
import { describeError } from './errors';

describe('describeError', () => {
  const stock = new ApiError(409, 'insufficient_stock', 'Not enough stock of Flagyl…', 'stock.short', {
    product: 'Flagyl 400mg Tablet',
    need: 150,
    have: 120,
    unit: 'tab',
    expired: 50,
  });

  it('keeps the server message in English', () => {
    expect(describeError(stock, 'en')).toBe('Not enough stock of Flagyl…');
  });

  it('translates known reasons into Urdu with the values filled in', () => {
    const text = describeError(stock, 'ur');
    expect(text).toContain('Flagyl 400mg Tablet');
    expect(text).toContain('150');
    expect(text).toContain('میعاد');
  });

  it('formats money in Urdu messages', () => {
    const e = new ApiError(403, 'approval_required', 'x', 'approval.refund', { limit: 50_000 });
    expect(describeError(e, 'ur')).toContain('Rs 500');
  });

  it('falls back to the code, then the English message', () => {
    expect(describeError(new ApiError(0, 'network', 'x'), 'ur')).toContain('نیٹ ورک');
    expect(describeError(new ApiError(400, 'bad_request', 'Something odd'), 'ur')).toBe('Something odd');
  });
});
