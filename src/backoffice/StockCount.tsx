import { useEffect, useRef, useState } from 'react';
import { api } from '../api/client';
import type { CountHead, CountLine, CountView, Product, RackProgress } from '../api/types';
import { SearchIcon } from '../components/common';
import { formatDateTime } from '../print/doc';
import { formatMonth } from '../domain/dates';
import { formatRs } from '../domain/money';
import { countingUnit, formatStock } from '../domain/stock';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import type { Page } from './Backoffice';

const REASONS: [string, StringKey][] = [
  ['damaged', 'adjDamaged'],
  ['expired', 'adjExpired'],
  ['lost', 'adjLost'],
  ['count', 'adjCount'],
  ['other', 'adjOther'],
];

/** Start a count, or pick up one in progress. */
export function StockCounts({ go }: { go: (p: Page) => void }) {
  const { t, lang } = useI18n();
  const [counts, setCounts] = useState<CountHead[] | null>(null);
  const [racks, setRacks] = useState<RackProgress[]>([]);
  const [section, setSection] = useState<string>('__all');
  const [blind, setBlind] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .counts()
      .then((r) => {
        setCounts(r.counts);
        setRacks(r.racks);
      })
      .catch((e) => setError(describeError(e, lang)));
  }, [lang]);

  const start = async () => {
    setError(null);
    try {
      const c = await api.startCount({
        section: section.startsWith('__') ? undefined : section,
        noRack: section === '__none',
        blind,
      });
      go({ name: 'count', countId: c.id });
    } catch (e) {
      setError(describeError(e, lang));
    }
  };

  const statusTag = (c: CountHead) =>
    c.status === 'open' ? (
      <span className="tag tag-warn">{t('countOpenBadge')}</span>
    ) : c.status === 'applied' ? (
      <span className="tag tag-ok">{t('countApplied')}</span>
    ) : (
      <span className="tag tag-mute">{t('countCancelled')}</span>
    );

  return (
    <>
      <div className="bo-head">
        <div>
          <span className="small">
            <button type="button" className="link-btn" onClick={() => go({ name: 'inventory' })}>
              {t('navInventory')}
            </button>{' '}
            › {t('stockCount')}
          </span>
          <h1>{t('countTitle')}</h1>
        </div>
      </div>
      <div className="cols">
        <section className="card grow">
          <span className="small">{t('countHelp')}</span>
          <div className="field">
            <label className="label" htmlFor="sc-section">{t('countWhat')}</label>
            <select id="sc-section" className="select" value={section} onChange={(e) => setSection(e.target.value)}>
              <option value="__all">
                {t('countWhole')} · {t('countBatches', { n: racks.reduce((s, r) => s + r.lines, 0) })}
              </option>
              {racks.map((r) =>
                r.rack ? (
                  <option key={r.rack} value={r.rack}>
                    {t('countRack', { rack: r.rack })} · {t('countBatches', { n: r.lines })}
                  </option>
                ) : (
                  <option key="__none" value="__none">
                    {t('countNoRack')} · {t('countBatches', { n: r.lines })}
                  </option>
                ),
              )}
            </select>
          </div>
          <label className="check">
            <input type="checkbox" checked={blind} onChange={(e) => setBlind(e.target.checked)} />
            {t('countBlind')}
          </label>
          {error && <div className="alert alert-bad">{error}</div>}
          <button type="button" className="btn btn-primary btn-lg" onClick={() => void start()}>
            {t('countStart')}
          </button>
        </section>
        <section className="card aside">
          <h2>{t('countHistory')}</h2>
          {counts?.length === 0 && <span className="muted small">—</span>}
          {counts?.map((c) => (
            <button
              key={c.id}
              type="button"
              className="list-row"
              style={{ border: 0, borderBottom: '1px solid var(--line-2)', background: 'transparent', width: '100%', textAlign: 'start' }}
              onClick={() => go({ name: 'count', countId: c.id })}
            >
              <div className="main">
                <strong className="mono">{c.countNo}</strong>
                <span className="small muted">
                  {c.section ? (c.section === '—' ? t('countNoRack') : t('countRack', { rack: c.section })) : t('countWhole')} ·{' '}
                  {c.startedBy} · <span className="ltr">{formatDateTime(c.startedAt)}</span>
                </span>
                <span className="tiny muted">{t('countProgress', { done: c.counted, total: c.lines })}</span>
              </div>
              {statusTag(c)}
            </button>
          ))}
        </section>
      </div>
    </>
  );
}

/** Splits base units into whole packs and loose units for the inputs. */
function split(line: CountLine, base: number | null): { packs: string; loose: string } {
  if (base === null) return { packs: '', loose: '' };
  if (line.sellMode === 'weight') return { packs: String(base / 1000), loose: '' };
  const pack = countingUnit(line.units);
  if (!pack) return { packs: '', loose: String(base) };
  return { packs: String(Math.floor(base / pack.baseQty)), loose: String(base % pack.baseQty) };
}

function join(line: CountLine, packs: string, loose: string): number | null {
  if (!packs.trim() && !loose.trim()) return null;
  if (line.sellMode === 'weight') {
    const kg = Number(packs);
    return Number.isFinite(kg) && kg >= 0 ? Math.round(kg * 1000) : NaN;
  }
  const pack = countingUnit(line.units);
  const p = packs.trim() ? Number(packs) : 0;
  const l = loose.trim() ? Number(loose) : 0;
  if (!Number.isInteger(p) || !Number.isInteger(l) || p < 0 || l < 0) return NaN;
  return p * (pack?.baseQty ?? 1) + l;
}

function CountRow({ line, open, onSave }: { line: CountLine; open: boolean; onSave: (counted: number | null, reason?: string) => Promise<void> }) {
  const { t } = useI18n();
  const initial = split(line, line.countedQty);
  const [packs, setPacks] = useState(initial.packs);
  const [loose, setLoose] = useState(initial.loose);
  const [bad, setBad] = useState(false);
  const pack = line.sellMode === 'weight' ? null : countingUnit(line.units);
  const fmt = (n: number) => formatStock(n, line.sellMode, line.baseUnit, line.units);

  const save = async () => {
    const v = join(line, packs, loose);
    if (v !== null && Number.isNaN(v)) return setBad(true);
    setBad(false);
    if (v === line.countedQty) return;
    await onSave(v, line.reason ?? undefined);
  };

  return (
    <tr>
      <td>
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <span className="pname" style={{ fontSize: 14 }}>{line.productName}</span>
          <span className="tiny muted ltr" style={{ textAlign: 'start' }}>
            {line.rack ?? '—'}
          </span>
        </div>
      </td>
      <td className="small ltr">
        {line.batchNo}
        {line.expiry ? ` · ${formatMonth(line.expiry)}` : ''}
      </td>
      <td className="small mono">{line.systemQty === null ? <span className="muted">{t('countHidden')}</span> : fmt(line.systemQty)}</td>
      <td style={{ width: 210 }}>
        {open ? (
          <div
            style={{ display: 'flex', gap: 6, alignItems: 'center', direction: 'ltr' }}
            // Save when leaving the row, not when moving between its two boxes.
            onBlur={(e) => {
              if (!e.currentTarget.contains(e.relatedTarget as Node | null)) void save();
            }}
          >
            <input
              aria-label={`${line.productName} ${pack?.name ?? (line.sellMode === 'weight' ? 'kg' : line.baseUnit)}`}
              className={`n${bad ? ' invalid' : ''}`}
              style={{ width: 70 }}
              inputMode="decimal"
              value={packs}
              onChange={(e) => setPacks(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && void save()}
            />
            <span className="tiny muted">{line.sellMode === 'weight' ? 'kg' : pack?.name ?? line.baseUnit}</span>
            {pack && (
              <>
                <input
                  aria-label={`${line.productName} ${line.baseUnit}`}
                  className={`n${bad ? ' invalid' : ''}`}
                  style={{ width: 56 }}
                  inputMode="numeric"
                  value={loose}
                  onChange={(e) => setLoose(e.target.value)}
                  onKeyDown={(e) => e.key === 'Enter' && void save()}
                />
                <span className="tiny muted">{line.baseUnit}</span>
              </>
            )}
          </div>
        ) : (
          <span className="small mono">{line.countedQty === null ? t('countNotCounted') : fmt(line.countedQty)}</span>
        )}
      </td>
      <td className="num small" style={{ color: line.difference ? (line.difference < 0 ? 'var(--bad)' : 'var(--warn)') : 'var(--brand)' }}>
        {line.difference === null ? '' : line.difference === 0 ? '✓' : `${line.difference > 0 ? '+' : ''}${fmt(line.difference)}`}
      </td>
      <td style={{ width: 170 }}>
        {line.difference ? (
          <select
            aria-label={t('colReason')}
            value={line.reason ?? ''}
            disabled={!open}
            onChange={(e) => void onSave(line.countedQty, e.target.value)}
          >
            <option value="">—</option>
            {REASONS.map(([r, k]) => (
              <option key={r} value={r}>
                {t(k)}
              </option>
            ))}
          </select>
        ) : null}
      </td>
    </tr>
  );
}

export function StockCountDetail({ countId, go }: { countId: string; go: (p: Page) => void }) {
  const { t, lang } = useI18n();
  const [c, setC] = useState<CountView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<Product[]>([]);
  const [rackFilter, setRackFilter] = useState<string | null | undefined>(undefined);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api
      .count(countId)
      .then(setC)
      .catch((e) => setError(describeError(e, lang)));
  }, [countId, lang]);

  useEffect(() => {
    const q = query.trim();
    if (!q) return setResults([]);
    const timer = setTimeout(() => api.searchProducts(q).then(setResults).catch(() => setResults([])), 200);
    return () => clearTimeout(timer);
  }, [query]);

  if (!c) return <div className="muted">{error ?? t('loading')}</div>;
  const open = c.status === 'open';
  const run = async (f: () => Promise<CountView>) => {
    setError(null);
    try {
      setC(await f());
    } catch (e) {
      setError(describeError(e, lang));
    }
  };
  const changed = c.items.filter((l) => l.difference).length;
  const shown = rackFilter === undefined ? c.items : c.items.filter((l) => l.rack === rackFilter);

  const addProduct = async (p: Product) => {
    setQuery('');
    setResults([]);
    await run(() => api.addCountProduct(c.id, p.id));
    searchRef.current?.focus();
  };

  return (
    <>
      <div className="bo-head">
        <div>
          <span className="small">
            <button type="button" className="link-btn" onClick={() => go({ name: 'counts' })}>
              {t('stockCount')}
            </button>{' '}
            › {c.countNo}
          </span>
          <h1>
            {c.countNo} · {c.section ? (c.section === '—' ? t('countNoRack') : t('countRack', { rack: c.section })) : t('countWhole')}
          </h1>
          <span className="sub">
            {c.startedBy} · <span className="ltr">{formatDateTime(c.startedAt)}</span> · {t('countProgress', { done: c.counted, total: c.lines })}
          </span>
        </div>
        {open && (
          <div className="bo-actions">
            <button
              type="button"
              className="btn btn-danger"
              onClick={() => window.confirm(t('countCancelConfirm')) && void run(() => api.cancelCount(c.id))}
            >
              {t('countCancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              disabled={c.counted === 0}
              onClick={() => {
                if (!window.confirm(t('countApplyConfirm', { n: changed }))) return;
                void run(() => api.applyCount(c.id)).then(() => setMessage(t('countDone', { no: c.countNo })));
              }}
            >
              {t('countApply')}
            </button>
          </div>
        )}
      </div>
      {message && <div className="alert alert-ok" role="status">{message}</div>}
      {error && <div className="alert alert-bad" role="alert">{error}</div>}

      <div className="kpis">
        <div className="kpi">
          <span className="label">{t('colCounted')}</span>
          <span className="value">
            {c.counted} / {c.lines}
          </span>
        </div>
        <div className="kpi">
          <span className="label">{t('colDiff')}</span>
          <span className="value">{changed}</span>
        </div>
        <div className="kpi">
          <span className="label">{t('countShort')}</span>
          <span className="value" style={{ color: c.shortValue ? 'var(--bad)' : undefined }}>{formatRs(c.shortValue)}</span>
        </div>
        <div className="kpi">
          <span className="label">{t('countExtra')}</span>
          <span className="value">{formatRs(c.extraValue)}</span>
        </div>
      </div>

      {c.racks.length > 1 && (
        <div className="pills" role="group">
          <button type="button" aria-pressed={rackFilter === undefined} onClick={() => setRackFilter(undefined)}>
            {t('tabAllProducts')}
          </button>
          {c.racks.map((r) => (
            <button key={r.rack ?? '—'} type="button" aria-pressed={rackFilter === r.rack} onClick={() => setRackFilter(r.rack)}>
              {r.rack ?? t('countNoRack')} · {r.counted}/{r.lines}
            </button>
          ))}
        </div>
      )}

      <section className="card" style={{ padding: 0, overflow: 'hidden' }}>
        {open && (
          <div style={{ padding: 14, borderBottom: '1px solid var(--line)' }}>
            <div className="searchbar" style={{ height: 46 }}>
              <SearchIcon />
              <label className="visually-hidden" htmlFor="sc-q">{t('countScan')}</label>
              <input
                id="sc-q"
                ref={searchRef}
                placeholder={t('countScan')}
                value={query}
                autoComplete="off"
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && results[0]) void addProduct(results[0]);
                }}
              />
            </div>
            {results.length > 0 && (
              <div className="result-list" style={{ marginTop: 6 }}>
                {results.map((p) => (
                  <button key={p.id} type="button" className="result" onClick={() => void addProduct(p)}>
                    <span className="names">
                      <span className="pname">{p.name}</span>
                    </span>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
        <div className="tbl-scroll">
          <table className="tbl" style={{ minWidth: 900 }}>
            <thead>
              <tr>
                <th>{t('colProduct')}</th>
                <th>{t('colBatch')}</th>
                <th>{t('colSystem')}</th>
                <th>{t('colCounted')}</th>
                <th className="num">{t('colDiff')}</th>
                <th>{t('colReason')}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((l) => (
                <CountRow key={l.id} line={l} open={open} onSave={(counted, reason) => run(() => api.setCountLine(c.id, l.id, counted, reason))} />
              ))}
            </tbody>
          </table>
        </div>
      </section>
      <span className="small muted">{t('countApplyNote')}</span>
    </>
  );
}
