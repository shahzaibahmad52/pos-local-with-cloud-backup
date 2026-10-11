import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { DashboardFigures, StoreFigures } from '../api/types';
import { periodRange, type Period } from '../domain/dates';
import { formatRs } from '../domain/money';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import { formatDateTime } from '../print/doc';
import { Modal } from './parts';

const PERIODS: [Period, StringKey][] = [
  ['today', 'periodToday'],
  ['week', 'periodWeek'],
  ['month', 'periodMonth'],
];

/** "3 min", "2 h", "5 days" since a time. */
function ago(iso: string): string {
  const mins = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 60_000));
  if (mins < 1) return '< 1 min';
  if (mins < 60) return `${mins} min`;
  if (mins < 48 * 60) return `${Math.round(mins / 60)} h`;
  return `${Math.round(mins / 1440)} days`;
}

export function HeadOffice() {
  const { t, lang } = useI18n();
  const [period, setPeriod] = useState<Period>('today');
  const [rows, setRows] = useState<StoreFigures[] | null>(null);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    const { from, to } = periodRange(period);
    setRows(null);
    api
      .hq(from, to)
      .then(setRows)
      .catch((e) => setError(describeError(e, lang)));
  };
  useEffect(load, [period, lang]);

  const total = (k: keyof DashboardFigures) => rows?.reduce((sum, r) => sum + (r.figures?.[k] ?? 0), 0) ?? 0;

  const remove = async (r: StoreFigures) => {
    if (!r.id || !window.confirm(t('hqConfirmRemove', { name: r.name }))) return;
    try {
      await api.removeHqStore(r.id);
      load();
    } catch (e) {
      setError(describeError(e, lang));
    }
  };

  return (
    <>
      <div className="bo-head">
        <div>
          <h1>{t('hqTitle')}</h1>
          <span className="sub">{t('hqSub')}</span>
        </div>
        <div className="bo-actions">
          <div role="group" className="seg" style={{ minWidth: 300 }}>
            {PERIODS.map(([p, k]) => (
              <button key={p} type="button" aria-pressed={period === p} onClick={() => setPeriod(p)}>
                {t(k)}
              </button>
            ))}
          </div>
          <button type="button" className="btn btn-primary" onClick={() => setAdding(true)}>
            {t('hqAdd')}
          </button>
        </div>
      </div>
      {error && <div className="alert alert-bad">{error}</div>}

      <section className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <div className="tbl-scroll">
          <table className="tbl" style={{ minWidth: 820 }}>
            <thead>
              <tr>
                <th>{t('colStore')}</th>
                <th className="num">{t('colNetSales')}</th>
                <th className="num">{t('colGross')}</th>
                <th className="num">{t('kpiExpenses')}</th>
                <th className="num">{t('colNet')}</th>
                <th className="num">{t('colKhata')}</th>
                <th>{t('colSynced')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows?.map((r) => {
                const stale = r.lastSyncAt && Date.now() - new Date(r.lastSyncAt).getTime() > 60 * 60_000;
                return (
                  <tr key={r.id ?? 'here'}>
                    <td>
                      <strong>{r.name}</strong> <span className="small muted ltr">{r.code}</span>
                      {r.thisStore && <span className="tag tag-mute" style={{ marginInlineStart: 6 }}>{t('hqThis')}</span>}
                      {r.error && <div className="small" style={{ color: 'var(--bad)' }}>{t('hqOffline', { msg: r.error })}</div>}
                      {r.figures && r.figures.bills > 0 && (
                        <div className="tiny muted">{t('kpiSalesSub', { bills: r.figures.bills, avg: formatRs(Math.round(r.figures.sales / r.figures.bills)) })}</div>
                      )}
                    </td>
                    <td className="num">{r.figures ? formatRs(r.figures.netSales) : '—'}</td>
                    <td className="num">{r.figures ? formatRs(r.figures.grossProfit) : '—'}</td>
                    <td className="num">{r.figures ? formatRs(r.figures.expenses) : '—'}</td>
                    <td className="num" style={{ color: r.figures && r.figures.netProfit < 0 ? 'var(--bad)' : undefined }}>
                      {r.figures ? formatRs(r.figures.netProfit) : '—'}
                    </td>
                    <td className="num">{formatRs(r.khataDue)}</td>
                    <td className="small" style={{ color: stale ? 'var(--warn)' : 'var(--brand)', fontWeight: stale ? 600 : undefined }}>
                      {r.lastSyncAt ? <span title={formatDateTime(r.lastSyncAt)}>{ago(r.lastSyncAt)}</span> : <span className="muted">{t('hqNoCloud')}</span>}
                    </td>
                    <td style={{ width: 90 }}>
                      {r.id && (
                        <button type="button" className="btn" style={{ height: 34, fontSize: 13 }} onClick={() => void remove(r)}>
                          {t('hqRemove')}
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
              {rows && rows.length > 1 && (
                <tr>
                  <td>
                    <strong>{t('hqTotal')}</strong>
                  </td>
                  <td className="num"><strong>{formatRs(total('netSales'))}</strong></td>
                  <td className="num"><strong>{formatRs(total('grossProfit'))}</strong></td>
                  <td className="num"><strong>{formatRs(total('expenses'))}</strong></td>
                  <td className="num"><strong>{formatRs(total('netProfit'))}</strong></td>
                  <td className="num"><strong>{formatRs(rows.reduce((s, r) => s + r.khataDue, 0))}</strong></td>
                  <td />
                  <td />
                </tr>
              )}
            </tbody>
          </table>
          {!rows && !error && <div className="empty">{t('loading')}</div>}
        </div>
      </section>
      <span className="small muted">{t('hqNote')}</span>

      {adding && (
        <AddStoreDialog
          onClose={() => setAdding(false)}
          onAdded={() => {
            setAdding(false);
            load();
          }}
        />
      )}
    </>
  );
}

function AddStoreDialog({ onClose, onAdded }: { onClose: () => void; onAdded: () => void }) {
  const { t, lang } = useI18n();
  const [url, setUrl] = useState('');
  const [token, setToken] = useState('');
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setBusy(true);
    try {
      await api.addHqStore({ url, token, name: name.trim() || undefined });
      onAdded();
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  return (
    <Modal title={t('hqAdd')} onClose={onClose}>
      <span className="small">{t('hqAddHelp')}</span>
      <div className="field">
        <label className="label" htmlFor="hq-url">{t('cloudUrl')}</label>
        <input id="hq-url" className="input ltr" placeholder="libsql://store-b2-yourname.turso.io" value={url} onChange={(e) => setUrl(e.target.value)} autoFocus />
      </div>
      <div className="field">
        <label className="label" htmlFor="hq-token">{t('cloudToken')}</label>
        <input id="hq-token" className="input ltr" type="password" autoComplete="off" value={token} onChange={(e) => setToken(e.target.value)} />
      </div>
      <div className="field">
        <label className="label" htmlFor="hq-name">{t('hqName')}</label>
        <input id="hq-name" className="input" value={name} onChange={(e) => setName(e.target.value)} />
      </div>
      {error && <div className="alert alert-bad">{error}</div>}
      <div style={{ display: 'flex', gap: 10 }}>
        <button type="button" className="btn" style={{ flex: 1 }} onClick={onClose}>
          {t('cancel')}
        </button>
        <button type="button" className="btn btn-primary" style={{ flex: 2 }} disabled={busy || !url.trim() || !token.trim()} onClick={() => void save()}>
          {busy ? t('loading') : t('save')}
        </button>
      </div>
    </Modal>
  );
}
