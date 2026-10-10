import { useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../api/client';
import type { MoneySource, Product, Purchase, Supplier } from '../api/types';
import { SearchIcon, TrashIcon } from '../components/common';
import { formatDate, parseExpiry, today } from '../domain/dates';
import { formatAmount, formatRs, parseRupees } from '../domain/money';
import { costPerBase, lineMoney, splitFreight } from '../domain/purchase';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import { local, MoneySourcePicker, newClientRef, useOpenTills } from './parts';
import { SupplierDialog, Suppliers } from './Suppliers';

type Tab = 'new' | 'history' | 'suppliers';

interface Line {
  key: string;
  productId: string;
  name: string;
  kind: 'medicine' | 'general';
  sellMode: 'unit' | 'weight';
  baseUnit: string;
  packs: { id: string; name: string; baseQty: number }[];
  unitId: string; // '' for weight items (bought per kg)
  batchNo: string;
  expiry: string;
  qty: string;
  bonus: string;
  tp: string;
  disc: string;
  mrp: string;
  /** Last trade price paid per pack, as a hint. */
  lastTp: { price: number; unit: string } | null;
}

interface Draft {
  supplierId: string;
  invoiceNo: string;
  invoiceDate: string;
  termsDays: string;
  freight: string;
  paidNow: string;
  paidFrom: MoneySource;
  till: string;
  lines: Line[];
  clientRef: string;
}

const DRAFT_KEY = 'bo.purchaseDraft';

function emptyDraft(): Draft {
  return {
    supplierId: '',
    invoiceNo: '',
    invoiceDate: today(),
    termsDays: '0',
    freight: '',
    paidNow: '',
    paidFrom: 'owner',
    till: '',
    lines: [],
    clientRef: newClientRef(),
  };
}

const int = (s: string) => (/^\d+$/.test(s.trim()) ? Number(s.trim()) : null);
const pct = (s: string) => {
  const n = Number(s.trim() || '0');
  return Number.isFinite(n) && n >= 0 && n <= 100 ? Math.round(n * 100) : null;
};

function packOf(l: Line) {
  return l.sellMode === 'weight' ? { name: 'kg', baseQty: 1000 } : l.packs.find((p) => p.id === l.unitId) ?? { name: '', baseQty: 1 };
}

export function Purchases({ tillCode, initialTab }: { tillCode: string; initialTab?: Tab }) {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>(initialTab ?? 'new');
  const tabs: [Tab, string][] = [
    ['new', t('tabNewPurchase')],
    ['history', t('tabHistory')],
    ['suppliers', t('tabSuppliers')],
  ];
  return (
    <>
      <div className="bo-head">
        <h1>{t('purchTitle')}</h1>
        <div className="pills" role="group" aria-label={t('purchTitle')}>
          {tabs.map(([k, label]) => (
            <button key={k} type="button" aria-pressed={tab === k} onClick={() => setTab(k)}>
              {label}
            </button>
          ))}
        </div>
      </div>
      {tab === 'new' && <NewPurchase tillCode={tillCode} />}
      {tab === 'history' && <History />}
      {tab === 'suppliers' && <Suppliers tillCode={tillCode} />}
    </>
  );
}

function NewPurchase({ tillCode }: { tillCode: string }) {
  const { t, lang } = useI18n();
  const [draft, setDraftState] = useState<Draft>(() => local.get<Draft>(DRAFT_KEY) ?? emptyDraft());
  const [suppliers, setSuppliers] = useState<Supplier[]>([]);
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<Product[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [addingSupplier, setAddingSupplier] = useState(false);
  const tills = useOpenTills();
  const searchRef = useRef<HTMLInputElement>(null);

  const setDraft = (d: Draft) => {
    setDraftState(d);
    local.set(DRAFT_KEY, d.lines.length || d.supplierId ? d : null);
  };
  const patch = (p: Partial<Draft>) => setDraft({ ...draft, ...p });
  const setLine = (key: string, p: Partial<Line>) => patch({ lines: draft.lines.map((l) => (l.key === key ? { ...l, ...p } : l)) });

  useEffect(() => {
    api
      .suppliers()
      .then(setSuppliers)
      .catch((e) => setError(describeError(e, lang)));
  }, [lang]);

  useEffect(() => {
    const q = query.trim();
    if (!q) return setResults([]);
    const timer = setTimeout(() => {
      api
        .searchProducts(q)
        .then(setResults)
        .catch(() => setResults([]));
    }, 200);
    return () => clearTimeout(timer);
  }, [query]);

  const addProduct = async (p: Product, unitId?: string | null) => {
    setQuery('');
    setResults([]);
    const packs = p.units.map((u) => ({ id: u.id, name: u.name, baseQty: u.baseQty }));
    const biggest = [...packs].sort((a, b) => b.baseQty - a.baseQty)[0];
    const line: Line = {
      key: newClientRef(),
      productId: p.id,
      name: p.name,
      kind: p.kind,
      sellMode: p.sellMode,
      baseUnit: p.baseUnit,
      packs,
      unitId: p.sellMode === 'weight' ? '' : unitId ?? biggest?.id ?? '',
      batchNo: '',
      expiry: '',
      qty: '1',
      bonus: '0',
      tp: '',
      disc: '0',
      mrp: '',
      lastTp: null,
    };
    try {
      const last = await api.lastBuy(p.id);
      if (last) {
        const samePack = p.sellMode === 'weight' ? last.unitName === 'kg' : packs.find((u) => u.name === last.unitName);
        if (samePack && typeof samePack === 'object') line.unitId = samePack.id;
        if (samePack) {
          line.tp = formatAmount(last.tradePrice).replace(/,/g, '');
          line.disc = String(last.discBp / 100);
        }
        line.lastTp = { price: last.tradePrice, unit: last.unitName };
      }
    } catch {
      // no history is fine
    }
    setDraftState((d) => {
      const next = { ...d, lines: [...d.lines, line] };
      local.set(DRAFT_KEY, next);
      return next;
    });
    searchRef.current?.focus();
  };

  const onSearchKey = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    const q = query.trim();
    if (!q) return;
    if (/^\d{6,}$/.test(q)) {
      try {
        const hit = await api.byBarcode(q);
        return void addProduct(hit.product, hit.unitId);
      } catch {
        // not a barcode: fall through to the first search result
      }
    }
    if (results[0]) void addProduct(results[0]);
  };

  // Money, worked out like the server does.
  const calc = useMemo(() => {
    const money = draft.lines.map((l) => lineMoney(int(l.qty) ?? 0, parseRupees(l.tp) ?? 0, pct(l.disc) ?? 0));
    const freight = parseRupees(draft.freight || '0') ?? 0;
    const shares = splitFreight(
      money.map((m) => m.total),
      freight,
    );
    const gross = money.reduce((s, m) => s + m.gross, 0);
    const discount = money.reduce((s, m) => s + m.discount, 0);
    const net = money.reduce((s, m) => s + m.total, 0) + freight;
    return { money, shares, gross, discount, freight, net };
  }, [draft]);

  const supplier = suppliers.find((s) => s.id === draft.supplierId);

  const lineProblem = (l: Line): string | null => {
    if (int(l.qty) === null || int(l.qty) === 0 || int(l.bonus) === null) return 'qty';
    if (parseRupees(l.tp) === null || pct(l.disc) === null) return 'tp';
    if (l.mrp.trim() && parseRupees(l.mrp) === null) return 'mrp';
    if (l.expiry.trim() && !parseExpiry(l.expiry)) return 'expiry';
    if (l.kind === 'medicine' && !l.expiry.trim()) return 'expiry';
    if (l.sellMode === 'unit' && !l.unitId) return 'pack';
    return null;
  };

  const receive = async () => {
    setError(null);
    setMessage(null);
    if (!draft.supplierId) return setError(t('chooseSupplier'));
    if (!draft.lines.length) return setError(t('noLines'));
    const bad = draft.lines.findIndex((l) => lineProblem(l));
    if (bad >= 0) return setError(`${t('rowN', { n: bad + 1 })}: ${t('fixFirst')}`);
    const paidNow = parseRupees(draft.paidNow || '0');
    if (paidNow === null || parseRupees(draft.freight || '0') === null) return setError(t('fixFirst'));
    setBusy(true);
    try {
      const p = await api.receivePurchase({
        supplierId: draft.supplierId,
        invoiceNo: draft.invoiceNo.trim() || undefined,
        invoiceDate: draft.invoiceDate,
        termsDays: int(draft.termsDays) ?? 0,
        freight: calc.freight,
        paidNow,
        paidFrom: draft.paidFrom,
        tillCode: draft.paidFrom === 'drawer' ? draft.till || tillCode : undefined,
        clientRef: draft.clientRef,
        lines: draft.lines.map((l) => ({
          productId: l.productId,
          unitId: l.sellMode === 'weight' ? undefined : l.unitId,
          batchNo: l.batchNo.trim(),
          expiry: parseExpiry(l.expiry) ?? undefined,
          qty: int(l.qty) ?? 0,
          bonus: int(l.bonus) ?? 0,
          tradePrice: parseRupees(l.tp) ?? 0,
          discBp: pct(l.disc) ?? 0,
          salePrice: l.mrp.trim() ? parseRupees(l.mrp) ?? undefined : undefined,
        })),
      });
      setMessage(t('purchaseSaved', { no: p.purchaseNo }));
      setDraft(emptyDraft());
      api.suppliers().then(setSuppliers).catch(() => undefined);
    } catch (e) {
      setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {message && <div className="alert alert-ok" role="status">{message}</div>}
      <section className="card">
        <div className="form-grid">
          <div className="field">
            <label className="label" htmlFor="pu-sup">{t('supplier')}</label>
            <select
              id="pu-sup"
              className="select"
              value={draft.supplierId}
              onChange={(e) => {
                if (e.target.value === '__new') return setAddingSupplier(true);
                const s = suppliers.find((x) => x.id === e.target.value);
                patch({ supplierId: e.target.value, termsDays: String(s?.termsDays ?? 0) });
              }}
            >
              <option value="">{t('chooseSupplier')}</option>
              {suppliers.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                  {s.balance > 0 ? ` — ${t('balanceOf', { amount: formatRs(s.balance) })}` : ''}
                </option>
              ))}
              <option value="__new">{t('newSupplierBtn')}</option>
            </select>
          </div>
          <div className="field">
            <label className="label" htmlFor="pu-inv">{t('invoiceNo')}</label>
            <input id="pu-inv" className="input ltr" value={draft.invoiceNo} onChange={(e) => patch({ invoiceNo: e.target.value })} />
          </div>
          <div className="field">
            <label className="label" htmlFor="pu-date">{t('invoiceDate')}</label>
            <input id="pu-date" type="date" className="input ltr" value={draft.invoiceDate} max={today()} onChange={(e) => patch({ invoiceDate: e.target.value })} />
          </div>
          <div className="field">
            <label className="label" htmlFor="pu-terms">{t('terms')}</label>
            <select id="pu-terms" className="select" value={draft.termsDays} onChange={(e) => patch({ termsDays: e.target.value })}>
              {Array.from(new Set(['0', '7', '15', '30', '45', '60', String(supplier?.termsDays ?? 0), draft.termsDays])).map((d) => (
                <option key={d} value={d}>
                  {d === '0' ? t('termsCash') : t('termsDays', { n: d })}
                </option>
              ))}
            </select>
          </div>
        </div>
      </section>

      <section className="card" style={{ position: 'relative' }}>
        <div className="searchbar">
          <SearchIcon />
          <label className="visually-hidden" htmlFor="pu-q">{t('addItem')}</label>
          <input
            id="pu-q"
            ref={searchRef}
            placeholder={t('addItem')}
            value={query}
            autoComplete="off"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => void onSearchKey(e)}
          />
        </div>
        {results.length > 0 && (
          <div className="result-list" role="listbox" aria-label={t('addItem')}>
            {results.map((p) => (
              <button key={p.id} type="button" role="option" aria-selected={false} className="result" onClick={() => void addProduct(p)}>
                <span className="names">
                  <span className="pname">{p.name}</span>
                  <span className="small muted">{[p.genericName, p.company].filter(Boolean).join(' · ')}</span>
                </span>
              </button>
            ))}
          </div>
        )}

        {draft.lines.length > 0 && (
          <div className="tbl-scroll">
            <table className="tbl" style={{ minWidth: 1180 }}>
              <thead>
                <tr>
                  <th>{t('colProduct')}</th>
                  <th>{t('colPack')}</th>
                  <th>{t('colBatch')}</th>
                  <th>{t('colExpiry')}</th>
                  <th className="num">{t('colQtyShort')}</th>
                  <th className="num">{t('colBonus')}</th>
                  <th className="num">{t('colTradePrice')}</th>
                  <th className="num">{t('colDiscPct')}</th>
                  <th className="num">{t('colMrp')}</th>
                  <th className="num">{t('colLineTotal')}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {draft.lines.map((l, i) => {
                  const pack = packOf(l);
                  const m = calc.money[i];
                  const cost = costPerBase(m, calc.shares[i], int(l.qty) ?? 0, int(l.bonus) ?? 0, pack.baseQty) * pack.baseQty;
                  const mrp = parseRupees(l.mrp);
                  const problem = lineProblem(l);
                  return (
                    <tr key={l.key}>
                      <td style={{ minWidth: 200 }}>
                        <div style={{ display: 'flex', flexDirection: 'column' }}>
                          <span className="pname" style={{ fontSize: 14 }}>{l.name}</span>
                          {l.lastTp && (
                            <span className="tiny muted">{t('lastBuyHint', { price: formatRs(l.lastTp.price), unit: l.lastTp.unit })}</span>
                          )}
                        </div>
                      </td>
                      <td style={{ width: 130 }}>
                        {l.sellMode === 'weight' ? (
                          <span className="small">kg</span>
                        ) : (
                          <select aria-label={t('colPack')} value={l.unitId} onChange={(e) => setLine(l.key, { unitId: e.target.value })}>
                            {l.packs.map((p) => (
                              <option key={p.id} value={p.id}>
                                {p.name}
                                {p.baseQty > 1 ? ` (${p.baseQty})` : ''}
                              </option>
                            ))}
                          </select>
                        )}
                      </td>
                      <td style={{ width: 110 }}>
                        <input aria-label={t('colBatch')} className="ltr" value={l.batchNo} onChange={(e) => setLine(l.key, { batchNo: e.target.value })} />
                      </td>
                      <td style={{ width: 120 }}>
                        <input
                          aria-label={t('colExpiry')}
                          className={`ltr${problem === 'expiry' && l.expiry ? ' invalid' : ''}`}
                          placeholder="MM/YYYY"
                          value={l.expiry}
                          onChange={(e) => setLine(l.key, { expiry: e.target.value })}
                        />
                      </td>
                      <td style={{ width: 80 }}>
                        <input aria-label={t('colQtyShort')} className="n" inputMode="numeric" value={l.qty} onChange={(e) => setLine(l.key, { qty: e.target.value })} />
                      </td>
                      <td style={{ width: 70 }}>
                        <input aria-label={t('colBonus')} className="n" inputMode="numeric" value={l.bonus} onChange={(e) => setLine(l.key, { bonus: e.target.value })} />
                      </td>
                      <td style={{ width: 110 }}>
                        <input aria-label={t('colTradePrice')} className="n" inputMode="decimal" value={l.tp} onChange={(e) => setLine(l.key, { tp: e.target.value })} />
                      </td>
                      <td style={{ width: 70 }}>
                        <input aria-label={t('colDiscPct')} className="n" inputMode="decimal" value={l.disc} onChange={(e) => setLine(l.key, { disc: e.target.value })} />
                      </td>
                      <td style={{ width: 110 }}>
                        <input
                          aria-label={t('colMrp')}
                          className={`n${mrp !== null && mrp > 0 && cost > mrp ? ' invalid' : ''}`}
                          inputMode="decimal"
                          disabled={l.sellMode === 'weight'}
                          value={l.mrp}
                          onChange={(e) => setLine(l.key, { mrp: e.target.value })}
                          title={mrp !== null && cost > mrp ? t('belowCost') : undefined}
                        />
                      </td>
                      <td className="num">
                        <div style={{ display: 'flex', flexDirection: 'column' }}>
                          <span>{formatAmount(m.total)}</span>
                          {cost > 0 && <span className="tiny muted">{t('costEach', { cost: formatAmount(cost), unit: pack.name })}</span>}
                        </div>
                      </td>
                      <td style={{ width: 48 }}>
                        <button type="button" className="icon-btn" aria-label={t('remove')} onClick={() => patch({ lines: draft.lines.filter((x) => x.key !== l.key) })}>
                          <TrashIcon />
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <span className="help">{t('bonusHelp')}</span>
      </section>

      <div className="cols">
        <div className="grow" />
        <section className="card aside">
          <div className="stat">
            <span>{t('grossAmount')}</span>
            <span className="mono">{formatRs(calc.gross)}</span>
          </div>
          <div className="stat">
            <span>{t('lineDiscounts')}</span>
            <span className="mono">− {formatRs(calc.discount)}</span>
          </div>
          <div className="row">
            <label htmlFor="pu-freight" className="small">{t('freight')}</label>
            <input id="pu-freight" className="input sm ltr" style={{ width: 130, textAlign: 'end' }} inputMode="decimal" value={draft.freight} onChange={(e) => patch({ freight: e.target.value })} />
          </div>
          <div className="total-row">
            <span style={{ fontWeight: 700 }}>{t('netPayable')}</span>
            <span className="total-amount" style={{ fontSize: 28 }}>{formatRs(calc.net)}</span>
          </div>
          <div className="row">
            <label htmlFor="pu-paid" className="small">{t('paidNow')}</label>
            <input id="pu-paid" className="input sm ltr" style={{ width: 130, textAlign: 'end' }} inputMode="decimal" value={draft.paidNow} onChange={(e) => patch({ paidNow: e.target.value })} />
          </div>
          {(parseRupees(draft.paidNow || '0') ?? 0) > 0 && (
            <MoneySourcePicker
              id="pu-from"
              label={t('paidFrom')}
              value={draft.paidFrom}
              onChange={(v) => patch({ paidFrom: v })}
              till={draft.till}
              onTill={(v) => patch({ till: v })}
              tills={tills}
            />
          )}
          {error && <div className="alert alert-bad" role="alert">{error}</div>}
          <button type="button" className="btn btn-primary btn-lg" disabled={busy} onClick={() => void receive()}>
            {t('receiveStock')}
          </button>
        </section>
      </div>

      {addingSupplier && (
        <SupplierDialog
          supplier={null}
          onClose={() => setAddingSupplier(false)}
          onSaved={(s) => {
            setSuppliers([...suppliers, s].sort((a, b) => a.name.localeCompare(b.name)));
            patch({ supplierId: s.id, termsDays: String(s.termsDays) });
            setAddingSupplier(false);
          }}
        />
      )}
    </>
  );
}

function History() {
  const { t, lang } = useI18n();
  const [list, setList] = useState<Purchase[] | null>(null);
  const [open, setOpen] = useState<Purchase | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .purchases()
      .then(setList)
      .catch((e) => setError(describeError(e, lang)));
  }, [lang]);

  const show = (id: string) =>
    api
      .purchase(id)
      .then(setOpen)
      .catch((e) => setError(describeError(e, lang)));

  return (
    <div className="cols">
      <section className="card grow" style={{ padding: 0, overflow: 'hidden' }}>
        {error && <div className="alert alert-bad">{error}</div>}
        <div className="tbl-scroll">
          <table className="tbl" style={{ minWidth: 640 }}>
            <thead>
              <tr>
                <th>{t('colDate')}</th>
                <th>{t('colPurchaseNo')}</th>
                <th>{t('colSupplier')}</th>
                <th>{t('colInvoice')}</th>
                <th className="num">{t('colAmount2')}</th>
                <th>{t('colDue')}</th>
              </tr>
            </thead>
            <tbody>
              {list?.map((p) => (
                <tr key={p.id} className="pick" tabIndex={0} aria-selected={open?.id === p.id} onClick={() => void show(p.id)} onKeyDown={(e) => e.key === 'Enter' && void show(p.id)}>
                  <td className="small ltr">{formatDate(p.invoiceDate)}</td>
                  <td className="mono small">{p.purchaseNo}</td>
                  <td>{p.supplierName}</td>
                  <td className="small ltr">{p.invoiceNo ?? '—'}</td>
                  <td className="num">{formatAmount(p.net)}</td>
                  <td className="small ltr">{p.dueDate ? formatDate(p.dueDate) : t('termsCash')}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {list?.length === 0 && <div className="empty">{t('noPurchases')}</div>}
          {!list && !error && <div className="empty">{t('loading')}</div>}
        </div>
      </section>
      {open && (
        <aside className="card aside">
          <div className="card-head">
            <h2 className="mono">{open.purchaseNo}</h2>
            <span className="small muted">{formatDate(open.createdAt)} · {open.userName}</span>
          </div>
          <span>
            {open.supplierName}
            {open.invoiceNo ? ` · ${open.invoiceNo}` : ''}
          </span>
          {open.lines.map((l, i) => (
            <div key={i} className="list-row" style={{ alignItems: 'flex-start' }}>
              <div className="main">
                <span className="pname" style={{ fontSize: 14 }}>{l.productName}</span>
                <span className="tiny muted ltr" style={{ textAlign: 'start' }}>
                  {l.qty}
                  {l.bonus ? `+${l.bonus}` : ''} {l.unitName} × {formatAmount(l.tradePrice)}
                  {l.discBp ? ` −${l.discBp / 100}%` : ''} · {l.batchNo}
                  {l.expiry ? ` · ${l.expiry}` : ''}
                </span>
              </div>
              <span className="mono small">{formatAmount(l.lineTotal)}</span>
            </div>
          ))}
          <div className="stat">
            <span>{t('freight')}</span>
            <span className="mono">{formatAmount(open.freight)}</span>
          </div>
          <div className="stat">
            <strong>{t('netPayable')}</strong>
            <strong className="mono">{formatRs(open.net)}</strong>
          </div>
          {open.paidNow > 0 && (
            <div className="stat">
              <span>{t('paidNow')}</span>
              <span className="mono">{formatRs(open.paidNow)}</span>
            </div>
          )}
        </aside>
      )}
    </div>
  );
}

