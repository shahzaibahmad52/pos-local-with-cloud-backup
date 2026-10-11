import { useEffect, useMemo, useState } from 'react';
import { api } from '../api/client';
import type { AdjustReason, BatchDetail, InventoryPage, InventoryTab, ProductDetail, StoreHeader } from '../api/types';
import { SearchIcon } from '../components/common';
import { formatDate, formatMonth } from '../domain/dates';
import { formatAmount, formatRs, ltr } from '../domain/money';
import { countingUnit, formatStock } from '../domain/stock';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import type { Page } from './Backoffice';

const TABS: [InventoryTab, StringKey][] = [
  ['all', 'tabAllProducts'],
  ['low', 'tabLow'],
  ['expiring', 'tabExpiring'],
  ['expired', 'tabExpired'],
  ['medicine', 'tabMedicine'],
  ['general', 'tabGeneral'],
  ['archived', 'tabArchived'],
];

const PAGE_SIZE = 50;

/** Price of the pack the shop counts in, or per kg for weight items. */
function shownPrice(r: { sellMode: string; pricePerKg: number | null; units: { name: string; baseQty: number; price: number }[] }) {
  if (r.sellMode === 'weight') return r.pricePerKg !== null ? `${formatRs(r.pricePerKg)}/kg` : '—';
  const u = countingUnit(r.units) ?? r.units[0];
  return u ? `${formatRs(u.price)}/${u.name}` : '—';
}

function shownCost(r: { sellMode: string; avgCost: number; units: { name: string; baseQty: number }[]; stockBase: number }) {
  if (r.stockBase <= 0) return '—';
  if (r.sellMode === 'weight') return `${formatRs(r.avgCost * 1000)}/kg`;
  const u = countingUnit(r.units) ?? r.units[0];
  return u ? `${formatRs(r.avgCost * u.baseQty)}/${u.name}` : formatRs(r.avgCost);
}

export function Inventory({
  store,
  go,
  initialTab,
  initialSelected,
}: {
  store: StoreHeader;
  go: (p: Page) => void;
  initialTab?: InventoryTab;
  initialSelected?: string;
}) {
  const { t, lang } = useI18n();
  const [tab, setTab] = useState<InventoryTab>(initialTab ?? 'all');
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState('');
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<InventoryPage | null>(null);
  const [selected, setSelected] = useState<string | null>(initialSelected ?? null);
  const [detail, setDetail] = useState<ProductDetail | null>(null);
  const [adjusting, setAdjusting] = useState<BatchDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      api
        .inventory({ q: query, tab, category, offset, limit: PAGE_SIZE })
        .then((p) => {
          setPage(p);
          setError(null);
          if (!selected && p.rows[0]) setSelected(p.rows[0].id);
        })
        .catch((e) => setError(describeError(e, lang)));
    }, query ? 250 : 0);
    return () => clearTimeout(timer);
    // `selected` is only a default for the first load, so it isn't a dependency.
  }, [query, tab, category, offset, lang]);

  useEffect(() => {
    if (!selected) return setDetail(null);
    api
      .productDetail(selected)
      .then(setDetail)
      .catch((e) => setError(describeError(e, lang)));
  }, [selected, lang]);

  const reload = (d: ProductDetail) => {
    setDetail(d);
    setPage((p) => (p ? { ...p, rows: p.rows.map((r) => (r.id === d.id ? { ...r, ...d } : r)) } : p));
  };

  return (
    <>
      <div className="bo-head">
        <div>
          <h1>{t('invTitle')}</h1>
          {page && (
            <span className="sub">
              {store.name} · {t('invSub', { n: page.summary.productCount.toLocaleString('en-US'), value: ltr(formatRs(page.summary.stockValue)) })}
            </span>
          )}
        </div>
        <div className="bo-actions">
          <button type="button" className="btn" onClick={() => go({ name: 'counts' })}>
            {t('stockCount')}
          </button>
          <button type="button" className="btn" onClick={() => go({ name: 'transfers' })}>
            {t('branchTransfers')}
          </button>
          <button type="button" className="btn" onClick={() => go({ name: 'import' })}>
            {t('importExcel')}
          </button>
          <button type="button" className="btn btn-primary" onClick={() => go({ name: 'product', productId: null })}>
            {t('addProduct')}
          </button>
        </div>
      </div>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10, alignItems: 'center' }}>
        <div className="pills" role="group" aria-label={t('invTitle')}>
          {TABS.map(([k, label]) => {
            const count =
              k === 'low' ? page?.summary.lowCount : k === 'expiring' ? page?.summary.expiringCount : k === 'expired' ? page?.summary.expiredCount : undefined;
            return (
              <button
                key={k}
                type="button"
                aria-pressed={tab === k}
                onClick={() => {
                  setTab(k);
                  setOffset(0);
                }}
              >
                {t(label)}
                {count ? ` (${count})` : ''}
              </button>
            );
          })}
        </div>
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10 }}>
        <div className="searchbar" style={{ flex: '3 1 320px', height: 48 }}>
          <SearchIcon />
          <label className="visually-hidden" htmlFor="inv-q">
            {t('searchProducts')}
          </label>
          <input
            id="inv-q"
            placeholder={t('searchProducts')}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setOffset(0);
            }}
          />
        </div>
        <label className="visually-hidden" htmlFor="inv-cat">
          {t('category')}
        </label>
        <select
          id="inv-cat"
          className="select"
          style={{ flex: '1 1 200px' }}
          value={category}
          onChange={(e) => {
            setCategory(e.target.value);
            setOffset(0);
          }}
        >
          <option value="">{t('allCategories')}</option>
          {page?.categories.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </div>

      {error && <div className="alert alert-bad">{error}</div>}

      <div className="cols">
        <section className="card grow" style={{ padding: 0, overflow: 'hidden' }}>
          <div className="tbl-scroll">
            <table className="tbl" style={{ minWidth: 700 }}>
              <thead>
                <tr>
                  <th>{t('colProduct')}</th>
                  <th>{t('colRack')}</th>
                  <th>{t('colStock')}</th>
                  <th className="num">{t('colCost')}</th>
                  <th className="num">{t('colSale')}</th>
                  <th>{t('colNextExpiry')}</th>
                </tr>
              </thead>
              <tbody>
                {page?.rows.map((r) => (
                  <tr
                    key={r.id}
                    className="pick"
                    aria-selected={r.id === selected}
                    tabIndex={0}
                    onClick={() => setSelected(r.id)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        setSelected(r.id);
                      }
                    }}
                  >
                    <td>
                      <div style={{ display: 'flex', flexDirection: 'column' }}>
                        <span className="pname">{r.name}</span>
                        <span className="small muted ltr" style={{ textAlign: 'start' }}>
                          {[r.genericName, r.company].filter(Boolean).join(' · ') || r.category || ''}
                        </span>
                      </div>
                    </td>
                    <td className="small ltr">{r.rack ?? '—'}</td>
                    <td>
                      <span className="mono small">{formatStock(r.stockBase, r.sellMode, r.baseUnit, r.units)}</span>{' '}
                      {r.low && <span className="tag tag-bad">{t('chipLow')}</span>}{' '}
                      {r.expiredBase > 0 && <span className="tag tag-bad">{t('expiredChip')}</span>}
                    </td>
                    <td className="num small">{shownCost(r)}</td>
                    <td className="num small">{shownPrice(r)}</td>
                    <td>
                      {r.nextExpiry ? (
                        <span className={r.expiring ? 'tag tag-warn' : 'small ltr'}>{formatMonth(r.nextExpiry)}</span>
                      ) : (
                        '—'
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {page?.rows.length === 0 && <div className="empty">{t('noProducts')}</div>}
            {!page && !error && <div className="empty">{t('loading')}</div>}
          </div>
          {page && page.total > 0 && (
            <div className="row" style={{ padding: '10px 14px', borderTop: '1px solid var(--line)' }}>
              <span className="small muted">
                {t('showingRange', { from: offset + 1, to: Math.min(offset + PAGE_SIZE, page.total), total: page.total.toLocaleString('en-US') })}
              </span>
              <div style={{ display: 'flex', gap: 8 }}>
                <button type="button" className="btn" style={{ height: 38 }} disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}>
                  {t('prevPage')}
                </button>
                <button type="button" className="btn" style={{ height: 38 }} disabled={offset + PAGE_SIZE >= page.total} onClick={() => setOffset(offset + PAGE_SIZE)}>
                  {t('nextPage')}
                </button>
              </div>
            </div>
          )}
        </section>

        <aside className="card aside" aria-label={t('colProduct')} style={{ flex: '1 1 320px' }}>
          {!detail && <span className="muted">{t('pickProduct')}</span>}
          {detail && <ProductPanel d={detail} onEdit={() => go({ name: 'product', productId: detail.id })} onAdjust={setAdjusting} />}
        </aside>
      </div>

      {adjusting && detail && (
        <AdjustDialog
          batch={adjusting}
          detail={detail}
          onClose={() => setAdjusting(null)}
          onSaved={(d) => {
            reload(d);
            setAdjusting(null);
          }}
        />
      )}
    </>
  );
}

function ProductPanel({ d, onEdit, onAdjust }: { d: ProductDetail; onEdit: () => void; onAdjust: (b: BatchDetail) => void }) {
  const { t } = useI18n();
  const sellable = d.allUnits.filter((u) => u.sellable);
  const pack = countingUnit(d.units) ?? d.units[0];
  const marginPct =
    d.sellMode === 'weight'
      ? d.pricePerKg && d.avgCost
        ? ((d.pricePerKg - d.avgCost * 1000) / d.pricePerKg) * 100
        : null
      : pack && d.avgCost
        ? ((pack.price - d.avgCost * pack.baseQty) / pack.price) * 100
        : null;
  const unitName = (id: string) => d.allUnits.find((u) => u.id === id)?.name ?? '';
  const moveKey = (reason: string): StringKey =>
    (['opening', 'purchase', 'adjust', 'return'].includes(reason) ? `move_${reason}` : 'move_adjust') as StringKey;

  return (
    <>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
        <h2 className="pname" style={{ fontSize: 19 }}>
          {d.name} {!d.active && <span className="tag tag-mute">{t('tabArchived')}</span>}
        </h2>
        <span className="small muted ltr" style={{ textAlign: 'start' }}>
          {[d.genericName, d.company, d.category].filter(Boolean).join(' · ')}
        </span>
      </div>

      <div className="box box-pad" style={{ background: 'var(--surface-2)' }}>
        <span className="small muted">{t('packsAndPrices')}</span>
        {d.sellMode === 'weight' ? (
          <span className="mono">{d.pricePerKg !== null ? `${formatRs(d.pricePerKg)} / kg` : '—'}</span>
        ) : (
          <span className="ltr small" style={{ textAlign: 'start' }}>
            {sellable.map((u) => `${u.name}${u.baseQty > 1 ? ` (${u.baseQty} ${d.baseUnit})` : ''} ${formatRs(d.units.find((x) => x.id === u.id)?.price ?? u.price)}`).join(' · ')}
          </span>
        )}
        {d.barcodes.length > 0 && (
          <span className="small muted ltr" style={{ textAlign: 'start' }}>
            {t('barcodesTitle')}: {d.barcodes.map((b) => `${b.code}${b.unitName ? ` (${b.unitName})` : ''}`).join(', ')}
          </span>
        )}
      </div>

      {d.expiredBase > 0 && (
        <div className="alert alert-bad small">{t('expiredOnShelf', { qty: ltr(formatStock(d.expiredBase, d.sellMode, d.baseUnit, d.units)) })}</div>
      )}

      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <span className="small muted">{t('batchesTitle')}</span>
        {d.batches.length === 0 && <span className="small">{t('noBatches')}</span>}
        {d.batches.map((b) => (
          <div key={b.id} className="list-row" style={{ gap: 8 }}>
            <div className="main">
              <span className="mono small" style={{ fontWeight: 600 }}>
                {b.batchNo}
              </span>
              <span className="tiny muted ltr" style={{ textAlign: 'start' }}>
                {b.expiry ? `${t('colExpiry')} ${formatMonth(b.expiry)}` : ''} · {t('colCost')} {formatAmount(b.costPerBase)}/{d.baseUnit}
                {b.prices.length > 0 && ` · ${b.prices.map(([uid, p]) => `${unitName(uid)} ${formatAmount(p)}`).join(', ')}`}
              </span>
            </div>
            <span className={`small mono ${b.expired ? 'tag tag-bad' : ''}`}>{formatStock(b.qtyOnHand, d.sellMode, d.baseUnit, d.units)}</span>
            <button type="button" className="btn" style={{ height: 34, padding: '0 10px', fontSize: 13 }} onClick={() => onAdjust(b)}>
              {t('correctStock')}
            </button>
          </div>
        ))}
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        <div className="stat">
          <span>{t('reorderLevel')}</span>
          <span className="mono">{d.reorderLevel ? formatStock(d.reorderLevel, d.sellMode, d.baseUnit, d.units) : '—'}</span>
        </div>
        <div className="stat">
          <span>{t('margin')}</span>
          <span className="mono">{marginPct === null ? '—' : `${marginPct.toFixed(1)}%`}</span>
        </div>
        <div className="stat">
          <span>{t('salesTax')}</span>
          <span>{d.taxRateBp ? t('taxIncludedPct', { pct: d.taxRateBp / 100 }) : t('taxExempt')}</span>
        </div>
        {d.kind === 'medicine' && (
          <div className="stat">
            <span>{t('prescription')}</span>
            <span>{d.rxRequired ? t('rxRequiredYes') : t('rxNotRequired')}</span>
          </div>
        )}
        {d.lastBuy && (
          <div className="stat">
            <span>{t('lastBought')}</span>
            <span className="small" style={{ textAlign: 'end' }}>
              {t('lastBoughtText', {
                unit: d.lastBuy.unitName,
                price: ltr(formatRs(d.lastBuy.tradePrice)),
                supplier: d.lastBuy.supplierName,
                date: ltr(formatDate(d.lastBuy.invoiceDate)),
              })}
            </span>
          </div>
        )}
      </div>

      {d.moves.length > 0 && (
        <details>
          <summary className="small" style={{ cursor: 'pointer' }}>
            {t('recentMoves')}
          </summary>
          {d.moves.map((m, i) => (
            <div key={i} className="list-row small">
              <span>
                {formatDate(m.createdAt)} · {t(moveKey(m.reason))}
                {m.batchNo ? ` · ${m.batchNo}` : ''}
              </span>
              <span className="mono" style={{ color: m.qtyDelta < 0 ? 'var(--bad)' : undefined }}>
                {m.qtyDelta > 0 ? '+' : ''}
                {m.qtyDelta.toLocaleString('en-US')} {d.baseUnit}
              </span>
            </div>
          ))}
        </details>
      )}

      <button type="button" className="btn btn-primary" onClick={onEdit}>
        {t('editProduct')}
      </button>
    </>
  );
}

const REASONS: [AdjustReason, StringKey][] = [
  ['damaged', 'adjDamaged'],
  ['expired', 'adjExpired'],
  ['lost', 'adjLost'],
  ['count', 'adjCount'],
  ['other', 'adjOther'],
];

function AdjustDialog({
  batch,
  detail,
  onClose,
  onSaved,
}: {
  batch: BatchDetail;
  detail: ProductDetail;
  onClose: () => void;
  onSaved: (d: ProductDetail) => void;
}) {
  const { t, lang } = useI18n();
  const unit = detail.sellMode === 'weight' ? 'g' : detail.baseUnit;
  const [qty, setQty] = useState(String(batch.qtyOnHand));
  const [reason, setReason] = useState<AdjustReason>(batch.expired ? 'expired' : 'damaged');
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const newQty = useMemo(() => (/^\d+$/.test(qty.trim()) ? Number(qty.trim()) : null), [qty]);
  const delta = newQty === null ? null : newQty - batch.qtyOnHand;

  const save = async () => {
    if (newQty === null || delta === 0) return;
    setBusy(true);
    try {
      onSaved(await api.adjustStock({ batchId: batch.id, newQty, reason, note: note.trim() || undefined }));
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  return (
    <div className="overlay" role="dialog" aria-modal="true" aria-labelledby="adj-title" onKeyDown={(e) => e.key === 'Escape' && onClose()}>
      <div className="dialog">
        <h2 id="adj-title">{t('adjustTitle', { batch: batch.batchNo })}</h2>
        <span className="pname">{detail.name}</span>
        <div className="field">
          <label className="label" htmlFor="adj-qty">
            {t('adjustNow', { unit })}
          </label>
          <input id="adj-qty" className="input big ltr" inputMode="numeric" value={qty} onChange={(e) => setQty(e.target.value)} autoFocus />
          {delta !== null && delta !== 0 && (
            <span className="help" style={{ color: delta < 0 ? 'var(--bad)' : 'var(--brand)' }}>
              {t('adjustChange', { delta: `${delta > 0 ? '+' : ''}${delta.toLocaleString('en-US')}`, unit })}
            </span>
          )}
        </div>
        <div className="field">
          <span className="label">{t('adjustWhy')}</span>
          <div className="grid-2" style={{ gap: 8, gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))' }}>
            {REASONS.map(([r, label]) => (
              <button key={r} type="button" className="choice" aria-pressed={reason === r} onClick={() => setReason(r)}>
                {t(label)}
              </button>
            ))}
          </div>
        </div>
        <div className="field">
          <label className="label" htmlFor="adj-note">
            {t('adjustNote')}
          </label>
          <input id="adj-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
        </div>
        {error && <div className="alert alert-bad">{error}</div>}
        <div style={{ display: 'flex', gap: 10 }}>
          <button type="button" className="btn" style={{ flex: 1 }} onClick={onClose}>
            {t('cancel')}
          </button>
          <button type="button" className="btn btn-primary" style={{ flex: 2 }} disabled={busy || newQty === null || delta === 0} onClick={() => void save()}>
            {t('saveCorrection')}
          </button>
        </div>
      </div>
    </div>
  );
}
