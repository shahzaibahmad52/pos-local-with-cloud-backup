import { useEffect, useMemo, useState } from 'react';
import { api } from '../api/client';
import type { ProductDetail, ProductInput } from '../api/types';
import { TrashIcon } from '../components/common';
import { inStoreBarcode } from '../domain/barcode';
import { parseExpiry } from '../domain/dates';
import { formatGrams, formatRs, parseRupees } from '../domain/money';
import { amountForGrams, gramsForAmount, roundDiv } from '../domain/pricing';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { Page } from './Backoffice';

interface Level {
  id?: string;
  name: string;
  nameUr: string;
  holds: string;
  price: string;
  sellable: boolean;
}

interface Form {
  kind: 'medicine' | 'general';
  sellMode: 'unit' | 'weight';
  name: string;
  genericName: string;
  company: string;
  category: string;
  rack: string;
  tax: string; // percent
  rx: boolean;
  controlled: boolean;
  cold: boolean;
  baseUnit: string;
  levels: Level[];
  pricePerKg: string;
  barcodes: { code: string; unitName: string }[];
  reorder: string; // in the counting pack, or kg for weight items
  reorderPack: number; // index into levels
  active: boolean;
  openQty: string;
  openPack: number;
  openBatch: string;
  openExpiry: string;
  openCost: string; // Rs per chosen pack (or per kg)
}

const rs = (paisa: number) => (paisa % 100 === 0 ? String(paisa / 100) : (paisa / 100).toFixed(2));

function blank(kind: Form['kind'], category = ''): Form {
  return {
    kind,
    sellMode: 'unit',
    name: '',
    genericName: '',
    company: '',
    category,
    rack: '',
    tax: kind === 'medicine' ? '0' : '18',
    rx: false,
    controlled: false,
    cold: false,
    baseUnit: kind === 'medicine' ? 'Tab' : 'Pc',
    levels:
      kind === 'medicine'
        ? [
            { name: 'Tab', nameUr: 'گولی', holds: '1', price: '', sellable: true },
            { name: 'Strip', nameUr: 'پتہ', holds: '10', price: '', sellable: true },
          ]
        : [{ name: 'Pc', nameUr: 'عدد', holds: '1', price: '', sellable: true }],
    pricePerKg: '',
    barcodes: [{ code: '', unitName: '' }],
    reorder: '',
    reorderPack: kind === 'medicine' ? 1 : 0,
    active: true,
    openQty: '',
    openPack: kind === 'medicine' ? 1 : 0,
    openBatch: '',
    openExpiry: '',
    openCost: '',
  };
}

function fromDetail(d: ProductDetail): Form {
  const levels: Level[] = d.allUnits.map((u) => ({
    id: u.id,
    name: u.name,
    nameUr: u.nameUr ?? '',
    holds: String(u.baseQty),
    price: rs(u.price),
    sellable: u.sellable,
  }));
  const reorderPack = Math.max(
    0,
    levels.findIndex((l) => Number(l.holds) > 1 && d.reorderLevel % Number(l.holds) === 0),
  );
  const holds = Number(levels[reorderPack]?.holds ?? 1);
  return {
    ...blank(d.kind),
    kind: d.kind,
    sellMode: d.sellMode,
    name: d.name,
    genericName: d.genericName ?? '',
    company: d.company ?? '',
    category: d.category ?? '',
    rack: d.rack ?? '',
    tax: String(d.taxRateBp / 100),
    rx: d.rxRequired,
    controlled: d.controlled,
    cold: d.coldChain,
    baseUnit: d.baseUnit,
    levels,
    pricePerKg: d.pricePerKg !== null ? rs(d.pricePerKg) : '',
    barcodes: d.barcodes.length ? d.barcodes.map((b) => ({ code: b.code, unitName: b.unitName ?? '' })) : [{ code: '', unitName: '' }],
    reorder: d.reorderLevel
      ? d.sellMode === 'weight'
        ? String(d.reorderLevel / 1000)
        : String(d.reorderLevel / (levels.length ? holds : 1))
      : '',
    reorderPack: levels.length ? reorderPack : 0,
    active: d.active,
  };
}

/** Problems with the form, by field name. Empty when it can be saved. */
function check(f: Form, isNew: boolean): Record<string, true> {
  const bad: Record<string, true> = {};
  if (!f.name.trim()) bad.name = true;
  const pct = Number(f.tax);
  if (!Number.isFinite(pct) || pct < 0 || pct > 100) bad.tax = true;
  if (f.sellMode === 'weight') {
    if (!(parseRupees(f.pricePerKg) ?? 0)) bad.pricePerKg = true;
  } else {
    if (!f.baseUnit.trim()) bad.baseUnit = true;
    const names = new Set<string>();
    f.levels.forEach((l, i) => {
      const key = l.name.trim().toLowerCase();
      if (!key || names.has(key)) bad[`lname${i}`] = true;
      names.add(key);
      if (!/^\d+$/.test(l.holds) || Number(l.holds) < 1) bad[`lholds${i}`] = true;
      if (parseRupees(l.price) === null) bad[`lprice${i}`] = true;
    });
    if (!f.levels.some((l) => l.sellable)) bad.levels = true;
  }
  if (f.reorder.trim() && !(Number(f.reorder) >= 0)) bad.reorder = true;
  if (isNew && f.openQty.trim()) {
    if (!(Number(f.openQty) > 0)) bad.openQty = true;
    if (f.openExpiry.trim() && !parseExpiry(f.openExpiry)) bad.openExpiry = true;
    if (f.kind === 'medicine' && !f.openExpiry.trim()) bad.openExpiry = true;
    if (f.openCost.trim() && parseRupees(f.openCost) === null) bad.openCost = true;
  }
  return bad;
}

function toInput(f: Form): ProductInput {
  const holdsOf = (i: number) => Number(f.levels[i]?.holds ?? 1) || 1;
  const reorder = Number(f.reorder || 0);
  return {
    name: f.name.trim(),
    genericName: f.genericName.trim() || null,
    company: f.company.trim() || null,
    category: f.category.trim() || null,
    kind: f.kind,
    sellMode: f.sellMode,
    baseUnit: f.sellMode === 'weight' ? 'g' : f.baseUnit.trim(),
    pricePerKg: f.sellMode === 'weight' ? parseRupees(f.pricePerKg) : null,
    taxRateBp: Math.round(Number(f.tax) * 100),
    rxRequired: f.kind === 'medicine' && f.rx,
    controlled: f.kind === 'medicine' && f.controlled,
    coldChain: f.cold,
    rack: f.rack.trim() || null,
    reorderLevel: Math.round(f.sellMode === 'weight' ? reorder * 1000 : reorder * holdsOf(f.reorderPack)),
    units:
      f.sellMode === 'weight'
        ? []
        : f.levels.map((l) => ({
            id: l.id,
            name: l.name.trim(),
            nameUr: l.nameUr.trim() || null,
            baseQty: Number(l.holds),
            price: parseRupees(l.price) ?? 0,
            sellable: l.sellable,
          })),
    barcodes: f.barcodes.filter((b) => b.code.trim()).map((b) => ({ code: b.code.trim(), unitName: b.unitName || null })),
    active: f.active,
  };
}

export function ProductForm({ productId, go }: { productId: string | null; go: (p: Page) => void }) {
  const { t, lang } = useI18n();
  const isNew = productId === null;
  const [form, setForm] = useState<Form | null>(isNew ? blank('medicine') : null);
  const [categories, setCategories] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [showErrors, setShowErrors] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .inventory({ limit: 1 })
      .then((p) => setCategories(p.categories))
      .catch(() => undefined);
    if (productId) {
      api
        .productDetail(productId)
        .then((d) => setForm(fromDetail(d)))
        .catch((e) => setError(describeError(e, lang)));
    }
  }, [productId, lang]);

  const bad = useMemo(() => (form ? check(form, isNew) : {}), [form, isNew]);
  if (!form) return <div className="muted">{error ?? t('loading')}</div>;

  const set = (patch: Partial<Form>) => setForm({ ...form, ...patch });
  const setLevel = (i: number, patch: Partial<Level>) => set({ levels: form.levels.map((l, j) => (j === i ? { ...l, ...patch } : l)) });
  const invalid = (k: string) => (showErrors && bad[k] ? ' invalid' : '');
  const packNames = form.levels.map((l) => l.name.trim()).filter(Boolean);
  const unitLabel = (i: number) => form.levels[i]?.name || form.baseUnit;

  const save = async (andNew: boolean) => {
    setShowErrors(true);
    setMessage(null);
    if (Object.keys(bad).length) return setError(t('fixFirst'));
    setError(null);
    setBusy(true);
    try {
      const input = toInput(form);
      if (isNew) {
        const created = await api.createProduct(input);
        if (form.openQty.trim()) {
          const weight = form.sellMode === 'weight';
          const holds = weight ? 1000 : Number(form.levels[form.openPack]?.holds ?? 1);
          const baseQty = Math.round(Number(form.openQty) * holds);
          const costPerPack = parseRupees(form.openCost) ?? 0;
          await api.receiveStock({
            productId: created.id,
            batchNo: form.openBatch.trim() || '-',
            expiry: parseExpiry(form.openExpiry) ?? undefined,
            baseQty,
            costPerBase: roundDiv(costPerPack, holds),
            reason: 'opening',
          });
        }
        if (andNew) {
          setForm(blank(form.kind, form.category));
          setShowErrors(false);
          setMessage(t('productSaved', { name: created.name }));
          window.scrollTo(0, 0);
        } else {
          go({ name: 'inventory', selected: created.id });
        }
      } else {
        const saved = await api.updateProduct(productId, input);
        go({ name: 'inventory', selected: saved.id, tab: saved.active ? undefined : 'archived' });
      }
    } catch (e) {
      setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  const perKg = parseRupees(form.pricePerKg) ?? 0;
  const smallest = form.levels.find((l) => Number(l.holds) === 1);

  return (
    <>
      <div className="bo-head">
        <div>
          <span className="small">
            <button type="button" className="btn" style={{ height: 'auto', padding: 0, border: 0, background: 'none', color: 'var(--brand)' }} onClick={() => go({ name: 'inventory' })}>
              {t('navInventory')}
            </button>{' '}
            › {isNew ? t('productNew') : t('productEditTitle')}
          </span>
          <h1 className="pname" style={{ fontSize: 26 }}>
            {form.name || (isNew ? t('productNew') : '')}
          </h1>
        </div>
        <div className="bo-actions">
          <button type="button" className="btn" onClick={() => go({ name: 'inventory' })}>
            {t('cancel')}
          </button>
          {isNew && (
            <button type="button" className="btn" disabled={busy} onClick={() => void save(true)}>
              {t('saveAndNew')}
            </button>
          )}
          <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void save(false)}>
            {t('saveProduct')}
          </button>
        </div>
      </div>

      {message && <div className="alert alert-ok" role="status">{message}</div>}
      {error && <div className="alert alert-bad" role="alert">{error}</div>}

      <div className="cols">
        <div className="grow" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <section className="card">
            <h2>{t('basicDetails')}</h2>
            <div role="group" aria-label={t('basicDetails')} className="seg" style={{ maxWidth: 420 }}>
              {(['medicine', 'general'] as const).map((k) => (
                <button
                  key={k}
                  type="button"
                  aria-pressed={form.kind === k}
                  onClick={() => (isNew && !form.name ? setForm({ ...blank(k, form.category) }) : set({ kind: k }))}
                >
                  {k === 'medicine' ? t('typeMedicine') : t('typeGeneral')}
                </button>
              ))}
            </div>
            <div className="form-grid">
              <div className="field">
                <label className="label" htmlFor="p-name">{t('productName')}</label>
                <input id="p-name" className={`input ltr${invalid('name')}`} value={form.name} onChange={(e) => set({ name: e.target.value })} autoFocus={isNew} />
              </div>
              {form.kind === 'medicine' && (
                <div className="field">
                  <label className="label" htmlFor="p-generic">{t('genericName')}</label>
                  <input id="p-generic" className="input ltr" value={form.genericName} onChange={(e) => set({ genericName: e.target.value })} />
                </div>
              )}
              <div className="field">
                <label className="label" htmlFor="p-company">{t('company')}</label>
                <input id="p-company" className="input ltr" value={form.company} onChange={(e) => set({ company: e.target.value })} />
              </div>
              <div className="field">
                <label className="label" htmlFor="p-cat">{t('category')}</label>
                <input id="p-cat" className="input" list="p-cats" value={form.category} onChange={(e) => set({ category: e.target.value })} />
                <datalist id="p-cats">
                  {categories.map((c) => (
                    <option key={c} value={c} />
                  ))}
                </datalist>
              </div>
              <div className="field">
                <label className="label" htmlFor="p-rack">{t('rack')}</label>
                <input id="p-rack" className="input ltr" value={form.rack} onChange={(e) => set({ rack: e.target.value })} />
              </div>
              <div className="field">
                <label className="label" htmlFor="p-tax">{t('taxField')}</label>
                <select
                  id="p-tax"
                  className="select"
                  value={['0', '18'].includes(form.tax) ? form.tax : 'other'}
                  onChange={(e) => set({ tax: e.target.value === 'other' ? '' : e.target.value })}
                >
                  <option value="0">{t('taxNone')}</option>
                  <option value="18">{t('tax18')}</option>
                  <option value="other">%</option>
                </select>
                {!['0', '18'].includes(form.tax) && (
                  <input aria-label="%" className={`input sm ltr${invalid('tax')}`} inputMode="decimal" value={form.tax} onChange={(e) => set({ tax: e.target.value })} />
                )}
              </div>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px 22px' }}>
              {form.kind === 'medicine' && (
                <>
                  <label className="check">
                    <input type="checkbox" checked={form.rx} onChange={(e) => set({ rx: e.target.checked })} />
                    {t('rxRequired')}
                  </label>
                  <label className="check">
                    <input type="checkbox" checked={form.controlled} onChange={(e) => set({ controlled: e.target.checked })} />
                    {t('controlled')}
                  </label>
                </>
              )}
              <label className="check">
                <input type="checkbox" checked={form.cold} onChange={(e) => set({ cold: e.target.checked })} />
                {t('coldChain')}
              </label>
            </div>
          </section>

          <section className="card" aria-label={t('soldHow')}>
            <h2>{t('soldHow')}</h2>
            <div role="group" aria-label={t('soldHow')} className="seg" style={{ maxWidth: 560 }}>
              {(['unit', 'weight'] as const).map((m) => (
                <button key={m} type="button" aria-pressed={form.sellMode === m} disabled={!isNew} onClick={() => set({ sellMode: m })}>
                  {m === 'unit' ? t('byPacks') : t('byWeightKg')}
                </button>
              ))}
            </div>
            {!isNew && <span className="help">{t('soldHowFixed')}</span>}

            {form.sellMode === 'weight' ? (
              <>
                <div className="field" style={{ maxWidth: 260 }}>
                  <label className="label" htmlFor="p-kg">{t('pricePerKg')}</label>
                  <input id="p-kg" className={`input big ltr${invalid('pricePerKg')}`} inputMode="decimal" value={form.pricePerKg} onChange={(e) => set({ pricePerKg: e.target.value })} />
                </div>
                {perKg > 0 && (
                  <div className="box box-pad" style={{ background: 'var(--surface-2)' }}>
                    <span className="small muted">{t('weightExamples')}</span>
                    <div className="form-grid ltr" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))', gap: 6 }}>
                      {[1000, 5000, 10000].map((a) => (
                        <span key={a} className="mono small">
                          {formatRs(a)} → {formatGrams(gramsForAmount(a, perKg))}
                        </span>
                      ))}
                      {[250, 500, 1000].map((g) => (
                        <span key={g} className="mono small">
                          {formatGrams(g)} → {formatRs(amountForGrams(g, perKg))}
                        </span>
                      ))}
                    </div>
                  </div>
                )}
              </>
            ) : (
              <>
                <span className="help">{t('packsHelp')}</span>
                <div className="field" style={{ maxWidth: 260 }}>
                  <label className="label" htmlFor="p-base">{t('smallestUnit')}</label>
                  <input id="p-base" className={`input ltr${invalid('baseUnit')}`} value={form.baseUnit} onChange={(e) => set({ baseUnit: e.target.value })} />
                </div>
                <div className="tbl-scroll">
                  <table className="tbl" style={{ minWidth: 640 }}>
                    <thead>
                      <tr>
                        <th>{t('levelName')}</th>
                        <th>{t('levelNameUr')}</th>
                        <th>{t('levelHolds')}</th>
                        <th>{t('levelPrice')}</th>
                        <th>{t('levelSell')}</th>
                        <th />
                      </tr>
                    </thead>
                    <tbody>
                      {form.levels.map((l, i) => (
                        <tr key={l.id ?? `new-${i}`}>
                          <td>
                            <input aria-label={t('levelName')} className={`ltr${invalid(`lname${i}`)}`} value={l.name} onChange={(e) => setLevel(i, { name: e.target.value })} />
                          </td>
                          <td>
                            <input aria-label={t('levelNameUr')} lang="ur" dir="rtl" value={l.nameUr} onChange={(e) => setLevel(i, { nameUr: e.target.value })} />
                          </td>
                          <td style={{ width: 120 }}>
                            <input aria-label={t('levelHolds')} className={`n${invalid(`lholds${i}`)}`} inputMode="numeric" value={l.holds} onChange={(e) => setLevel(i, { holds: e.target.value })} />
                          </td>
                          <td style={{ width: 140 }}>
                            <input aria-label={t('levelPrice')} className={`n${invalid(`lprice${i}`)}`} inputMode="decimal" value={l.price} onChange={(e) => setLevel(i, { price: e.target.value })} />
                          </td>
                          <td style={{ width: 60, textAlign: 'center' }}>
                            <input type="checkbox" aria-label={t('levelSell')} style={{ width: 18, height: 18 }} checked={l.sellable} onChange={(e) => setLevel(i, { sellable: e.target.checked })} />
                          </td>
                          <td style={{ width: 52 }}>
                            {form.levels.length > 1 && (
                              <button type="button" className="icon-btn" aria-label={t('remove')} onClick={() => set({ levels: form.levels.filter((_, j) => j !== i) })}>
                                <TrashIcon />
                              </button>
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10, alignItems: 'center' }}>
                  <button
                    type="button"
                    className="btn"
                    style={{ height: 40 }}
                    onClick={() => set({ levels: [...form.levels, { name: '', nameUr: '', holds: '', price: '', sellable: true }] })}
                  >
                    {t('addLevel')}
                  </button>
                  {smallest && parseRupees(smallest.price) !== null && (
                    <span className="small muted ltr">
                      {form.levels
                        .filter((l) => l.name && Number(l.holds) > 0 && parseRupees(l.price) !== null)
                        .map((l) => t('priceSummary', { unit: l.name, price: formatRs(parseRupees(l.price)!) }))
                        .join(' · ')}
                    </span>
                  )}
                </div>
                {showErrors && bad.levels && <div className="alert alert-bad small">{t('packsHelp')}</div>}
                {!isNew && <span className="help">{t('batchPricesNote')}</span>}
              </>
            )}
          </section>

          <section className="card">
            <h2>{t('barcodesTitle')}</h2>
            {form.barcodes.map((b, i) => (
              <div key={i} style={{ display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center' }}>
                <input
                  aria-label={t('barcodesTitle')}
                  className="input sm mono ltr"
                  style={{ flex: '2 1 200px' }}
                  value={b.code}
                  onChange={(e) => set({ barcodes: form.barcodes.map((x, j) => (j === i ? { ...x, code: e.target.value } : x)) })}
                />
                {form.sellMode === 'unit' && (
                  <select
                    aria-label={t('barcodeOn')}
                    className="select sm"
                    style={{ flex: '1 1 140px' }}
                    value={b.unitName}
                    onChange={(e) => set({ barcodes: form.barcodes.map((x, j) => (j === i ? { ...x, unitName: e.target.value } : x)) })}
                  >
                    <option value="">{t('barcodeOn')}: —</option>
                    {packNames.map((n) => (
                      <option key={n} value={n}>
                        {t('barcodeOn')}: {n}
                      </option>
                    ))}
                  </select>
                )}
                <button type="button" className="icon-btn" aria-label={t('remove')} onClick={() => set({ barcodes: form.barcodes.filter((_, j) => j !== i) })}>
                  <TrashIcon />
                </button>
              </div>
            ))}
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
              <button type="button" className="btn" style={{ height: 40 }} onClick={() => set({ barcodes: [...form.barcodes, { code: '', unitName: '' }] })}>
                {t('addBarcode')}
              </button>
              <button
                type="button"
                className="btn"
                style={{ height: 40 }}
                onClick={() => set({ barcodes: [...form.barcodes.filter((b) => b.code.trim()), { code: inStoreBarcode(), unitName: '' }] })}
              >
                {t('makeBarcode')}
              </button>
            </div>
          </section>
        </div>

        <div className="aside" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <section className="card">
            <div className="field">
              <label className="label" htmlFor="p-reorder">
                {t('reorderWhen', { unit: form.sellMode === 'weight' ? 'kg' : unitLabel(form.reorderPack) })}
              </label>
              <div style={{ display: 'flex', gap: 8 }}>
                <input id="p-reorder" className={`input ltr${invalid('reorder')}`} style={{ flex: 1 }} inputMode="decimal" value={form.reorder} onChange={(e) => set({ reorder: e.target.value })} />
                {form.sellMode === 'unit' && form.levels.length > 1 && (
                  <select aria-label={t('levelName')} className="select" style={{ flex: 1 }} value={form.reorderPack} onChange={(e) => set({ reorderPack: Number(e.target.value) })}>
                    {form.levels.map((l, i) => (
                      <option key={i} value={i}>
                        {l.name || '—'}
                      </option>
                    ))}
                  </select>
                )}
              </div>
            </div>
            {!isNew && (
              <>
                <label className="check">
                  <input type="checkbox" checked={form.active} onChange={(e) => set({ active: e.target.checked })} />
                  {t('activeProduct')}
                </label>
                {!form.active && <span className="help">{t('archivedNote')}</span>}
              </>
            )}
          </section>

          {isNew && (
            <section className="card">
              <h2>{t('openingTitle')}</h2>
              <span className="help">{t('openingHelp')}</span>
              <div className="field">
                <label className="label" htmlFor="p-oq">
                  {t('openingQty', { unit: form.sellMode === 'weight' ? 'kg' : unitLabel(form.openPack) })}
                </label>
                <div style={{ display: 'flex', gap: 8 }}>
                  <input id="p-oq" className={`input ltr${invalid('openQty')}`} style={{ flex: 1 }} inputMode="decimal" value={form.openQty} onChange={(e) => set({ openQty: e.target.value })} />
                  {form.sellMode === 'unit' && form.levels.length > 1 && (
                    <select aria-label={t('levelName')} className="select" style={{ flex: 1 }} value={form.openPack} onChange={(e) => set({ openPack: Number(e.target.value) })}>
                      {form.levels.map((l, i) => (
                        <option key={i} value={i}>
                          {l.name || '—'}
                        </option>
                      ))}
                    </select>
                  )}
                </div>
              </div>
              <div className="form-grid" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(130px, 1fr))' }}>
                <div className="field">
                  <label className="label" htmlFor="p-ob">{t('batchNo')}</label>
                  <input id="p-ob" className="input ltr" value={form.openBatch} onChange={(e) => set({ openBatch: e.target.value })} />
                </div>
                <div className="field">
                  <label className="label" htmlFor="p-oe">{t('expiryField')}</label>
                  <input id="p-oe" className={`input ltr${invalid('openExpiry')}`} placeholder="08/2028" value={form.openExpiry} onChange={(e) => set({ openExpiry: e.target.value })} />
                </div>
              </div>
              <div className="field">
                <label className="label" htmlFor="p-oc">
                  {t('costPerUnit', { unit: form.sellMode === 'weight' ? 'kg' : unitLabel(form.openPack) })}
                </label>
                <input id="p-oc" className={`input ltr${invalid('openCost')}`} inputMode="decimal" value={form.openCost} onChange={(e) => set({ openCost: e.target.value })} />
              </div>
            </section>
          )}
        </div>
      </div>
    </>
  );
}
