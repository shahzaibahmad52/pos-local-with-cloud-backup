import { describe, expect, it } from 'vitest';
import { addDays, formatDate, parseExpiry, periodRange, recentMonths } from './dates';

describe('dates', () => {
  it('adds days across months', () => {
    expect(addDays('2026-10-10', 30)).toBe('2026-11-09');
    expect(addDays('2026-03-01', -1)).toBe('2026-02-28');
  });

  it('makes dashboard periods', () => {
    expect(periodRange('today', '2026-10-10')).toEqual({ from: '2026-10-10', to: '2026-10-10' });
    expect(periodRange('week', '2026-10-10')).toEqual({ from: '2026-10-04', to: '2026-10-10' });
    expect(periodRange('month', '2026-10-10')).toEqual({ from: '2026-10-01', to: '2026-10-10' });
  });

  it('lists recent months', () => {
    expect(recentMonths(3, '2026-01-15')).toEqual(['2026-01', '2025-12', '2025-11']);
  });

  it('reads expiry the way it is printed on packs', () => {
    expect(parseExpiry('08/2028')).toBe('2028-08');
    expect(parseExpiry('8-28')).toBe('2028-08');
    expect(parseExpiry('2028-8')).toBe('2028-08');
    expect(parseExpiry('Aug 2028')).toBe('2028-08');
    expect(parseExpiry('13/2028')).toBeNull();
    expect(parseExpiry('')).toBeNull();
  });

  it('formats dates', () => {
    expect(formatDate('2026-10-10T14:00:00+05:00')).toBe('10 Oct 2026');
  });
});
