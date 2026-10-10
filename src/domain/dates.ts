// Dates as the store server stores them: `YYYY-MM-DD` in the shop's local time.

const pad = (n: number) => String(n).padStart(2, '0');

export function isoDate(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function today(): string {
  return isoDate(new Date());
}

export function addDays(date: string, days: number): string {
  const [y, m, d] = date.split('-').map(Number);
  return isoDate(new Date(y, m - 1, d + days));
}

export function monthOf(date: string): string {
  return date.slice(0, 7);
}

export type Period = 'today' | 'week' | 'month';

/** Today, the last 7 days, or this calendar month so far. */
export function periodRange(period: Period, now = today()): { from: string; to: string } {
  switch (period) {
    case 'today':
      return { from: now, to: now };
    case 'week':
      return { from: addDays(now, -6), to: now };
    case 'month':
      return { from: `${monthOf(now)}-01`, to: now };
  }
}

const MONTHS_EN = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** "10 Oct 2026" from `2026-10-10` (or a full timestamp). */
export function formatDate(date: string): string {
  const [y, m, d] = date.slice(0, 10).split('-').map(Number);
  if (!y || !m || !d) return date;
  return `${d} ${MONTHS_EN[m - 1]} ${y}`;
}

/** "Oct 2026" from `2026-10`. */
export function formatMonth(month: string): string {
  const [y, m] = month.split('-').map(Number);
  if (!y || !m) return month;
  return `${MONTHS_EN[m - 1]} ${y}`;
}

/** The last `count` months, newest first, as `YYYY-MM`. */
export function recentMonths(count: number, now = today()): string[] {
  const [y, m] = now.split('-').map(Number);
  return Array.from({ length: count }, (_, i) => {
    const d = new Date(y, m - 1 - i, 1);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}`;
  });
}

/** Turns "08/2028", "8-2028", "2028-08" or "Aug 2028" into `2028-08`; null if unreadable. */
export function parseExpiry(input: string): string | null {
  const s = input.trim();
  if (!s) return null;
  let m = /^(\d{4})[-/.](\d{1,2})$/.exec(s);
  if (m) return valid(Number(m[1]), Number(m[2]));
  m = /^(\d{1,2})[-/.](\d{2}|\d{4})$/.exec(s);
  if (m) return valid(m[2].length === 2 ? 2000 + Number(m[2]) : Number(m[2]), Number(m[1]));
  m = /^([a-z]{3})[a-z]*[\s-]*(\d{4})$/i.exec(s);
  if (m) {
    const idx = MONTHS_EN.findIndex((x) => x.toLowerCase() === m![1].toLowerCase());
    if (idx >= 0) return valid(Number(m[2]), idx + 1);
  }
  return null;
}

function valid(year: number, month: number): string | null {
  if (year < 2000 || year > 2100 || month < 1 || month > 12) return null;
  return `${year}-${pad(month)}`;
}
