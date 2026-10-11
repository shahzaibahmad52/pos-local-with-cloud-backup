import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { api } from '../api/client';
import type { Report, ReportColumn, StoreHeader } from '../api/types';
import { addDays, formatDate, formatMonth, monthOf, today } from '../domain/dates';
import { formatAmount } from '../domain/money';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import { formatDateTime } from '../print/doc';
import type { Page } from './Backoffice';

const GROUPS: [StringKey, string[]][] = [
  ['repGroupProfit', ['profit_loss', 'product_profit', 'discounts']],
  ['repGroupSales', ['daily_sales', 'sales_by_product', 'sales_by_category', 'sales_by_cashier', 'sales_by_payment', 'returns']],
  ['repGroupCash', ['shifts', 'cash_book']],
  ['repGroupStock', ['stock_valuation', 'stock_movement', 'expiry', 'slow_stock']],
  ['repGroupPurchases', ['purchase_register', 'payables']],
  ['repGroupCustomers', ['receivables_aging']],
  ['repGroupTax', ['sales_tax']],
];

/** Reports that show the stock or balances as they are now, not for a period. */
const AS_OF_NOW = ['stock_valuation', 'expiry', 'slow_stock', 'payables', 'receivables_aging'];

type Range = 'today' | 'yesterday' | 'week' | 'month' | 'lastMonth' | 'custom';

function rangeDates(r: Range): { from: string; to: string } {
  const now = today();
  switch (r) {
    case 'today':
      return { from: now, to: now };
    case 'yesterday': {
      const y = addDays(now, -1);
      return { from: y, to: y };
    }
    case 'week':
      return { from: addDays(now, -6), to: now };
    case 'lastMonth': {
      const firstThis = `${monthOf(now)}-01`;
      const lastPrev = addDays(firstThis, -1);
      return { from: `${monthOf(lastPrev)}-01`, to: lastPrev };
    }
    default:
      return { from: `${monthOf(now)}-01`, to: now };
  }
}

const METHODS: Record<string, StringKey> = {
  cash: 'cash',
  card: 'card',
  jazzcash: 'jazzcash',
  easypaisa: 'easypaisa',
  khata: 'khata',
};

/** A cell as the reader sees it. */
function useFormat() {
  const { t } = useI18n();
  return (col: ReportColumn, v: unknown): string => {
    if (v === null || v === undefined || v === '') return '—';
    switch (col.kind) {
      case 'money':
        return formatAmount(Number(v));
      case 'int':
        return Number(v).toLocaleString('en-US');
      case 'pct':
        return `${(Number(v) / 100).toFixed(1)}%`;
      case 'date':
        return formatDate(String(v));
      case 'month':
        return formatMonth(String(v));
      case 'datetime':
        return formatDateTime(String(v));
      default: {
        const s = String(v);
        if (s.startsWith('@')) return t(`rv_${s.slice(1)}` as StringKey);
        if (col.key === 'method' && METHODS[s]) return t(METHODS[s]);
        return s;
      }
    }
  };
}

const isNumeric = (c: ReportColumn) => c.kind === 'money' || c.kind === 'int' || c.kind === 'pct';

export function Reports({ store, go }: { store: StoreHeader; go: (p: Page) => void }) {
  const { t, lang } = useI18n();
  const fmt = useFormat();
  const [name, setName] = useState('profit_loss');
  const [range, setRange] = useState<Range>('month');
  const [custom, setCustom] = useState(rangeDates('month'));
  const [n, setN] = useState<Record<string, number>>({ expiry: 3, slow_stock: 60 });
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [printing, setPrinting] = useState(false);

  const dates = range === 'custom' ? custom : rangeDates(range);
  const asOfNow = AS_OF_NOW.includes(name);

  useEffect(() => {
    setReport(null);
    setError(null);
    api
      .report(name, dates.from, dates.to, n[name])
      .then(setReport)
      .catch((e) => setError(describeError(e, lang)));
  }, [name, dates.from, dates.to, n, lang]);

  const title = t(`rp_${name}` as StringKey);
  const period = asOfNow
    ? `${t('repAsOfNow')} · ${formatDate(today())}`
    : dates.from === dates.to
      ? formatDate(dates.from)
      : `${formatDate(dates.from)} – ${formatDate(dates.to)}`;

  const print = () => {
    setPrinting(true);
    // Reports print on A4 (receipts use 80 mm paper), only while printing.
    const page = document.createElement('style');
    page.textContent = '@page { size: A4; margin: 12mm; }';
    document.head.appendChild(page);
    // Let the print copy render first; remove it once printing is done.
    setTimeout(() => {
      const done = () => {
        setPrinting(false);
        page.remove();
        window.removeEventListener('afterprint', done);
      };
      window.addEventListener('afterprint', done);
      window.print();
    }, 80);
  };

  const exportExcel = async () => {
    if (!report) return;
    const { default: writeXlsxFile } = await import('write-excel-file/browser');
    const bold = (v: string) => ({ value: v, fontWeight: 'bold' as const });
    const value = (c: ReportColumn, v: unknown) => {
      if (v === null || v === undefined) return null;
      if (c.kind === 'money') return Number(v) / 100;
      if (c.kind === 'pct') return Number(v) / 100;
      if (c.kind === 'int') return Number(v);
      return fmt(c, v);
    };
    const head = [[bold(store.name)], [bold(title)], [period], []];
    const data =
      report.lines.length > 0
        ? report.lines.map((l) => [l.label ?? t(l.key as StringKey), l.style === 'heading' ? null : (l.style === 'less' ? -l.amount : l.amount) / 100])
        : [
            report.columns.map((c) => bold(t(`rc_${c.key}` as StringKey))),
            ...report.rows.map((r) => report.columns.map((c) => value(c, r[c.key]))),
            ...(report.totals
              ? [report.columns.map((c, i) => (i === 0 ? bold(t('repTotal')) : c.key in report.totals! ? value(c, report.totals![c.key]) : null))]
              : []),
          ];
    await writeXlsxFile([...head, ...data] as never, {
      columns: (report.lines.length ? [40, 16] : report.columns.map((c) => (c.kind === 'text' ? 28 : 14))).map((width) => ({ width })),
    }).toFile(`${name}-${dates.from}-${dates.to}.xlsx`);
  };

  const body = report && <ReportBody report={report} fmt={fmt} />;

  return (
    <>
      <div className="bo-head">
        <h1>{t('navReports')}</h1>
        <div className="bo-actions">
          <button type="button" className="btn" disabled={!report} onClick={print}>
            {t('repPrint')}
          </button>
          <button type="button" className="btn" disabled={!report} onClick={() => void exportExcel().catch((e) => setError(String(e)))}>
            {t('repExcel')}
          </button>
        </div>
      </div>

      <div className="cols">
        <nav className="card" aria-label={t('repTitle')} style={{ flex: '0 1 250px', gap: 14 }}>
          {GROUPS.map(([g, names]) => (
            <div key={g} style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
              <span className="tiny muted" style={{ fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.5 }}>
                {t(g)}
              </span>
              {names.map((r) => (
                <button
                  key={r}
                  type="button"
                  className="bo-link"
                  aria-current={r === name ? 'page' : undefined}
                  style={{ color: r === name ? '#fff' : 'var(--ink)', minHeight: 36, fontSize: 14 }}
                  onClick={() => setName(r)}
                >
                  {t(`rp_${r}` as StringKey)}
                </button>
              ))}
              {g === 'repGroupTax' && (
                <button type="button" className="bo-link" style={{ color: 'var(--ink)', minHeight: 36, fontSize: 14 }} onClick={() => go({ name: 'stores' })}>
                  {t('repBranches')} ›
                </button>
              )}
            </div>
          ))}
        </nav>

        <section className="card grow" style={{ flex: '4 1 600px' }}>
          <div className="card-head" style={{ alignItems: 'flex-start' }}>
            <div>
              <h2 style={{ fontSize: 20 }}>{title}</h2>
              <span className="small muted">
                {store.name} · <span className="ltr">{period}</span>
              </span>
            </div>
            {!asOfNow && (
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center' }}>
                <label className="visually-hidden" htmlFor="rep-range">{t('repFrom')}</label>
                <select id="rep-range" className="select sm" value={range} onChange={(e) => setRange(e.target.value as Range)}>
                  <option value="today">{t('periodToday')}</option>
                  <option value="yesterday">{t('repYesterday')}</option>
                  <option value="week">{t('periodWeek')}</option>
                  <option value="month">{t('periodMonth')}</option>
                  <option value="lastMonth">{t('repLastMonth')}</option>
                  <option value="custom">{t('repFrom')} … {t('repTo')}</option>
                </select>
                {range === 'custom' && (
                  <>
                    <input aria-label={t('repFrom')} type="date" className="input sm ltr" value={custom.from} max={custom.to} onChange={(e) => setCustom({ ...custom, from: e.target.value })} />
                    <input aria-label={t('repTo')} type="date" className="input sm ltr" value={custom.to} min={custom.from} max={today()} onChange={(e) => setCustom({ ...custom, to: e.target.value })} />
                  </>
                )}
              </div>
            )}
            {(name === 'expiry' || name === 'slow_stock') && (
              <label className="small" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                {name === 'expiry' ? t('repMonthsAhead') : t('repNoSaleFor')}
                <select
                  className="select sm"
                  value={n[name]}
                  onChange={(e) => setN({ ...n, [name]: Number(e.target.value) })}
                >
                  {(name === 'expiry' ? [1, 3, 6, 12] : [30, 60, 90, 180]).map((v) => (
                    <option key={v} value={v}>
                      {name === 'expiry' ? t('repMonthsN', { n: v }) : t('repDaysN', { n: v })}
                    </option>
                  ))}
                </select>
              </label>
            )}
          </div>
          {error && <div className="alert alert-bad">{error}</div>}
          {!report && !error && <div className="muted">{t('loading')}</div>}
          {body}
          {name === 'profit_loss' && <span className="tiny muted">{t('repPlNote')}</span>}
          {name === 'sales_tax' && <span className="tiny muted">{t('repTaxNote')}</span>}
        </section>
      </div>

      {printing &&
        report &&
        createPortal(
          <div className="print-root report-print" dir={lang === 'ur' ? 'rtl' : 'ltr'}>
            <h1>{store.name}</h1>
            <h2>{title}</h2>
            <p>{period}</p>
            <ReportBody report={report} fmt={fmt} print />
            <p className="foot">{t('repPrinted', { time: formatDateTime(new Date().toISOString()) })}</p>
          </div>,
          document.body,
        )}
    </>
  );
}

function ReportBody({ report, fmt, print = false }: { report: Report; fmt: (c: ReportColumn, v: unknown) => string; print?: boolean }) {
  const { t } = useI18n();
  if (report.lines.length > 0) {
    return (
      <div className="statement">
        {report.lines.map((l, i) => {
          const label = l.label ?? t(l.key as StringKey);
          if (l.style === 'heading') return <div key={i} className="st-heading">{label}</div>;
          const amount = l.style === 'less' ? `(${formatAmount(l.amount)})` : formatAmount(l.amount);
          return (
            <div key={i} className={`st-line st-${l.style}`}>
              <span>
                {label}
                {l.pct !== null && <span className="muted small"> · {t('pl_margin', { pct: (l.pct / 100).toFixed(1) })}</span>}
              </span>
              <span className="num" style={l.style === 'total' && l.amount < 0 ? { color: 'var(--bad)' } : undefined}>
                {l.style === 'total' ? `Rs ${amount}` : amount}
              </span>
            </div>
          );
        })}
      </div>
    );
  }
  if (report.rows.length === 0) return <div className="empty">{t('repEmpty')}</div>;
  return (
    <div className={print ? undefined : 'tbl-scroll'}>
      <table className="tbl report-table" style={print ? undefined : { minWidth: Math.max(520, report.columns.length * 96) }}>
        <thead>
          <tr>
            {report.columns.map((c) => (
              <th key={c.key} className={isNumeric(c) ? 'num' : undefined}>
                {t(`rc_${c.key}` as StringKey)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {report.rows.map((r, i) => (
            <tr key={i}>
              {report.columns.map((c) => {
                const v = r[c.key];
                const negative = isNumeric(c) && typeof v === 'number' && v < 0;
                return (
                  <td
                    key={c.key}
                    className={isNumeric(c) ? 'num' : c.key === 'product' || c.key === 'items' ? 'pname-cell' : undefined}
                    style={negative ? { color: 'var(--bad)' } : undefined}
                  >
                    {isNumeric(c) ? fmt(c, v) : <span dir="auto">{fmt(c, v)}</span>}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
        {report.totals && (
          <tfoot>
            <tr>
              {report.columns.map((c, i) => (
                <td key={c.key} className={isNumeric(c) ? 'num' : undefined} style={{ fontWeight: 700, borderTop: '2px solid var(--ink)' }}>
                  {i === 0 ? t('repTotal') : c.key in report.totals! ? fmt(c, report.totals![c.key]) : ''}
                </td>
              ))}
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  );
}
