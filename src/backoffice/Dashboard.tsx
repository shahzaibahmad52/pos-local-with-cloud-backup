import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { Dashboard as DashboardData, StoreHeader } from '../api/types';
import { formatDate, formatMonth, periodRange, type Period } from '../domain/dates';
import { formatRs, ltr } from '../domain/money';
import { formatStock } from '../domain/stock';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import type { Page } from './Backoffice';

const PERIODS: [Period, StringKey][] = [
  ['today', 'periodToday'],
  ['week', 'periodWeek'],
  ['month', 'periodMonth'],
];

const METHOD_KEYS: Record<string, StringKey> = {
  cash: 'cash',
  card: 'card',
  jazzcash: 'jazzcash',
  easypaisa: 'easypaisa',
  khata: 'khata',
};

/** Whole rupees with thousands separators, for chart labels. */
function shortRs(paisa: number): string {
  const rs = Math.round(paisa / 100);
  return rs >= 10_000 ? `${Math.round(rs / 1000)}k` : rs.toLocaleString('en-US');
}

export function Dashboard({ store, go }: { store: StoreHeader; go: (p: Page) => void }) {
  const { t, lang } = useI18n();
  const [period, setPeriod] = useState<Period>('today');
  const [data, setData] = useState<DashboardData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const { from, to } = periodRange(period);
    setError(null);
    api
      .dashboard(from, to)
      .then(setData)
      .catch((e) => setError(describeError(e, lang)));
  }, [period, lang]);

  const f = data?.figures;
  const pct = (part: number, whole: number) => (whole > 0 ? ((part / whole) * 100).toFixed(1) : '0');
  const max = Math.max(1, ...(data?.days.map((d) => d.netSales) ?? [1]));
  const rangeText = data
    ? data.from === data.to
      ? formatDate(data.from)
      : period === 'month'
        ? formatMonth(data.from.slice(0, 7))
        : `${formatDate(data.from)} – ${formatDate(data.to)}`
    : '';

  return (
    <>
      <div className="bo-head">
        <div>
          <h1>{t('navDashboard')}</h1>
          <span className="sub">
            {store.name} · <span className="ltr">{rangeText}</span>
          </span>
        </div>
        <div role="group" aria-label={t('navDashboard')} className="seg" style={{ minWidth: 300 }}>
          {PERIODS.map(([p, k]) => (
            <button key={p} type="button" aria-pressed={period === p} onClick={() => setPeriod(p)}>
              {t(k)}
            </button>
          ))}
        </div>
      </div>

      {error && <div className="alert alert-bad">{error}</div>}
      {!data && !error && <div className="muted">{t('loading')}</div>}

      {data && f && (
        <>
          <div className="kpis">
            <div className="kpi">
              <span className="label">{t('kpiSales')}</span>
              <span className="value">{formatRs(f.netSales)}</span>
              <span className="sub">
                {t('kpiSalesSub', { bills: f.bills, avg: ltr(formatRs(f.bills ? Math.round(f.sales / f.bills) : 0)) })}
                {f.refunds > 0 && <> · {t('kpiRefundsSub', { refunds: formatRs(f.refunds) })}</>}
              </span>
            </div>
            <div className="kpi">
              <span className="label">{t('kpiGross')}</span>
              <span className="value">{formatRs(f.grossProfit)}</span>
              <span className="sub">{t('kpiGrossSub', { pct: pct(f.grossProfit, f.netSales - f.salesTax) })}</span>
            </div>
            <div className="kpi">
              <span className="label">{t('kpiExpenses')}</span>
              <span className="value">{formatRs(f.expenses)}</span>
              <span className="sub">{t('kpiExpensesSub')}</span>
            </div>
            <div className="kpi">
              <span className="label">{t('kpiNet')}</span>
              <span className="value" style={{ color: f.netProfit < 0 ? 'var(--bad)' : 'var(--brand-dark)' }}>
                {formatRs(f.netProfit)}
              </span>
              <span className="sub">{t('kpiNetSub')}</span>
            </div>
            <button type="button" className="kpi" style={{ textAlign: 'start' }} onClick={() => go({ name: 'khata' })}>
              <span className="label">{t('kpiKhata')}</span>
              <span className="value">{formatRs(data.khataDue)}</span>
              <span className="sub">{t('kpiKhataSub', { n: data.khataCustomers })}</span>
            </button>
            <button type="button" className="kpi" style={{ textAlign: 'start' }} onClick={() => go({ name: 'purchases', tab: 'suppliers' })}>
              <span className="label">{t('kpiPayables')}</span>
              <span className="value">{formatRs(data.payables)}</span>
              <span className="sub" style={data.payablesOverdue ? { color: 'var(--warn)', fontWeight: 600 } : undefined}>
                {data.payablesOverdue ? t('kpiPayablesOverdue', { amount: formatRs(data.payablesOverdue) }) : t('kpiPayablesOk')}
              </span>
            </button>
          </div>
          {f.salesTax > 0 && <span className="small muted">{t('taxNote', { amount: formatRs(f.salesTax) })}</span>}

          <div className="cols">
            <section className="card grow">
              <div className="card-head">
                <h2>{t('chartTitle')}</h2>
                <span className="small muted">{t('chartUnit')}</span>
              </div>
              <div className="bars" role="img" aria-label={t('chartTitle')}>
                {data.days.map((d, i) => (
                  <div key={d.date} className={`bar${i === data.days.length - 1 ? ' today' : ''}`} title={`${formatDate(d.date)}: ${formatRs(d.netSales)}`}>
                    <span style={{ height: `${Math.max(0, (d.netSales / max) * 100)}%` }} />
                  </div>
                ))}
              </div>
              <div className="bar-days" aria-hidden="true">
                {data.days.map((d) => (
                  <span key={d.date}>{Number(d.date.slice(8, 10))}</span>
                ))}
              </div>
              <span className="tiny muted ltr" style={{ textAlign: 'end' }}>
                max {shortRs(max)}
              </span>
            </section>

            <section className="card aside">
              <h2>{t('paymentsTitle')}</h2>
              {data.payments.length === 0 && <span className="muted">{t('noSales')}</span>}
              {data.payments.map(([method, amount]) => (
                <div key={method} className="list-row">
                  <span>{t(METHOD_KEYS[method] ?? 'cash')}</span>
                  <span className="mono">{formatRs(amount)}</span>
                </div>
              ))}
            </section>
          </div>

          <div className="cols">
            <section className="card grow" style={{ flexBasis: 380 }}>
              <div className="card-head">
                <h2>{t('lowStockTitle')}</h2>
                <button type="button" className="btn" style={{ height: 38 }} onClick={() => go({ name: 'purchases', tab: 'new' })}>
                  {t('newPurchase')}
                </button>
              </div>
              {data.lowStock.length === 0 && <span className="muted">{t('nothingLow')}</span>}
              {data.lowStock.map((r) => (
                <button
                  key={r.id}
                  type="button"
                  className="list-row"
                  style={{ border: 0, borderBottom: '1px solid var(--line-2)', background: 'transparent', textAlign: 'start', width: '100%' }}
                  onClick={() => go({ name: 'inventory', tab: 'low', selected: r.id })}
                >
                  <div className="main">
                    <span className="pname">{r.name}</span>
                    <span className="small muted">{t('reorderAt', { n: ltr(formatStock(r.reorderLevel, r.sellMode, r.baseUnit, r.units)) })}</span>
                  </div>
                  <span className="tag tag-bad">{t('nLeft', { n: ltr(formatStock(r.stockBase, r.sellMode, r.baseUnit, r.units)) })}</span>
                </button>
              ))}
            </section>
            <section className="card aside" style={{ flexBasis: 380 }}>
              <div className="card-head">
                <h2>{t('expiringTitle')}</h2>
                <button type="button" className="btn" style={{ height: 38 }} onClick={() => go({ name: 'inventory', tab: 'expiring' })}>
                  {t('viewAll')}
                </button>
              </div>
              {data.expiring.length === 0 && <span className="muted">{t('nothingExpiring')}</span>}
              {data.expiring.map((b) => (
                <button
                  key={`${b.productId}-${b.batchNo}-${b.expiry}`}
                  type="button"
                  className="list-row"
                  style={{ border: 0, borderBottom: '1px solid var(--line-2)', background: 'transparent', textAlign: 'start', width: '100%' }}
                  onClick={() => go({ name: 'inventory', tab: b.expired ? 'expired' : 'expiring', selected: b.productId })}
                >
                  <div className="main">
                    <span className="pname">{b.productName}</span>
                    <span className="small muted">
                      {t('colBatch')} <span className="ltr">{b.batchNo} · {b.qtyBase.toLocaleString('en-US')} {b.baseUnit}</span>
                    </span>
                  </div>
                  <span className={`tag ${b.expired ? 'tag-bad' : 'tag-warn'}`}>
                    {b.expired ? t('expiredChip') : t('expChip', { date: ltr(formatMonth(b.expiry)) })}
                  </span>
                </button>
              ))}
            </section>
          </div>
        </>
      )}
    </>
  );
}
