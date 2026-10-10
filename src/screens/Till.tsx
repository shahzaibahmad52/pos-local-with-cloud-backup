import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import { createPortal } from 'react-dom';
import { api, ApiError, setToken } from '../api/client';
import type { Customer, Language, LoginTile, PaymentMethod, Product, Receipt, ReceiptLanguage, Shift, StoreHeader, User } from '../api/types';
import { BrandMark, LanguageToggle, SearchIcon, TrashIcon } from '../components/common';
import { Receipt80 } from '../components/Receipt80';
import { addToCart, baseQtyOnBill, cartTotals, priceLine, toSaleLines, type CartLine, type WeightLine } from '../domain/cart';
import { formatGrams, formatRs, parseRupees } from '../domain/money';
import { useI18n } from '../i18n/i18n';

const METHODS: PaymentMethod[] = ['cash', 'card', 'jazzcash', 'easypaisa', 'khata'];
const DISCOUNTS = [0, 500, 1000];

function newRef(): string {
  return typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function monthsUntil(expiry: string | null): number | null {
  if (!expiry) return null;
  const [y, m] = expiry.split('-').map(Number);
  const now = new Date();
  return (y - now.getFullYear()) * 12 + (m - (now.getMonth() + 1));
}

function readAutoPrint(): boolean {
  try {
    return window.localStorage.getItem('medstore.autoPrint') !== '0';
  } catch {
    return true;
  }
}

export function Till({
  user,
  shift,
  store,
  tillCode,
  onSignOut,
}: {
  user: User;
  shift: Shift;
  store: StoreHeader;
  tillCode: string;
  onSignOut: () => void;
}) {
  const { t, lang, setLang } = useI18n();
  const [lines, setLines] = useState<CartLine[]>([]);
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<Product[] | null>(null);
  const [highlight, setHighlight] = useState(0);
  const [customers, setCustomers] = useState<Customer[]>([]);
  const [customerId, setCustomerId] = useState('');
  const [addingCustomer, setAddingCustomer] = useState(false);
  const [discountBp, setDiscountBp] = useState(0);
  const [method, setMethod] = useState<PaymentMethod>('cash');
  const [tendered, setTendered] = useState('');
  const [clientRef, setClientRef] = useState(newRef);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [approvalNeeded, setApprovalNeeded] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [online, setOnline] = useState(true);
  const searchRef = useRef<HTMLInputElement>(null);

  const totals = useMemo(() => cartTotals(lines, discountBp), [lines, discountBp]);
  const tenderedPaisa = method === 'cash' ? (tendered.trim() === '' ? totals.total : parseRupees(tendered)) : totals.total;
  const change = tenderedPaisa === null ? null : tenderedPaisa - totals.total;
  const customer = customers.find((c) => c.id === customerId) ?? null;

  const focusSearch = () => searchRef.current?.focus();

  const loadCustomers = useCallback(() => {
    api.customers().then(setCustomers).catch(() => undefined);
  }, []);
  useEffect(loadCustomers, [loadCustomers]);

  // Connection to the main PC.
  useEffect(() => {
    let alive = true;
    const ping = () =>
      api
        .health()
        .then(() => alive && setOnline(true))
        .catch(() => alive && setOnline(false));
    const id = window.setInterval(ping, 15_000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  // Search as the cashier types.
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setResults(null);
      return;
    }
    const id = window.setTimeout(() => {
      api
        .searchProducts(q)
        .then((r) => {
          setResults(r);
          setHighlight(0);
        })
        .catch((e) => setError(e instanceof ApiError ? e.message : String(e)));
    }, 150);
    return () => window.clearTimeout(id);
  }, [query]);

  const add = (product: Product, unitId?: string | null) => {
    setLines((current) => addToCart(current, product, unitId));
    setQuery('');
    setResults(null);
    setError(null);
    focusSearch();
  };

  const onSearchEnter = async () => {
    const q = query.trim();
    if (!q) return;
    // Scanners type the digits and press Enter.
    if (/^\d{6,}$/.test(q)) {
      try {
        const hit = await api.byBarcode(q);
        add(hit.product, hit.unitId);
        return;
      } catch (e) {
        if (!(e instanceof ApiError && e.status === 404)) {
          setError(e instanceof ApiError ? e.message : String(e));
          return;
        }
      }
    }
    const list = results ?? (await api.searchProducts(q).catch(() => []));
    const pick = list[highlight] ?? list[0];
    if (pick) add(pick);
    else setError(t('noMatch'));
  };

  const update = (key: string, patch: Partial<CartLine>) =>
    setLines((current) =>
      current
        .map((l) => (l.key === key ? ({ ...l, ...patch } as CartLine) : l))
        .filter((l) => l.kind === 'weight' || l.qty > 0),
    );
  const remove = (key: string) => setLines((current) => current.filter((l) => l.key !== key));

  const resetBill = () => {
    setLines([]);
    setCustomerId('');
    setDiscountBp(0);
    setMethod('cash');
    setTendered('');
    setClientRef(newRef());
    setError(null);
    setReceipt(null);
    setApprovalNeeded(null);
    loadCustomers();
    window.setTimeout(focusSearch, 0);
  };

  const pay = async (approval?: { userId: string; secret: string }) => {
    if (lines.length === 0 || busy) return;
    setError(null);
    if (method === 'khata' && !customerId) return setError(t('khataNeedsCustomer'));
    if (tenderedPaisa === null || tenderedPaisa < totals.total) {
      return setError(t('shortBy', { amount: formatRs(totals.total - (tenderedPaisa ?? 0)) }));
    }
    setBusy(true);
    try {
      const r = await api.createSale({
        tillCode,
        customerId: customerId || undefined,
        lines: toSaleLines(lines),
        discountBp,
        payments: [{ method, amount: method === 'cash' ? tenderedPaisa : totals.total }],
        clientRef,
        approval,
      });
      setApprovalNeeded(null);
      setReceipt(r);
      if (readAutoPrint()) window.setTimeout(() => window.print(), 150);
    } catch (e) {
      if (e instanceof ApiError && e.code === 'approval_required') {
        setApprovalNeeded(e.message);
      } else {
        setError(e instanceof ApiError ? e.message : String(e));
      }
    } finally {
      setBusy(false);
    }
  };

  // Keyboard shortcuts from the design: F2 search, F12 pay, Esc clear.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'F2') {
        e.preventDefault();
        focusSearch();
      } else if (e.key === 'F12') {
        e.preventDefault();
        if (receipt) resetBill();
        else void pay();
      } else if (e.key === 'Escape') {
        if (receipt) resetBill();
        else if (!approvalNeeded) {
          setQuery('');
          setResults(null);
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const changeLanguage = (next: Language) => {
    setLang(next);
    api.setLanguage(next).catch(() => undefined);
  };

  const signOut = async () => {
    await api.logout().catch(() => undefined);
    setToken(null);
    onSignOut();
  };

  const shiftTime = new Date(shift.openedAt).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  const receiptLanguage: ReceiptLanguage = receipt?.customerReceiptLanguage ?? receipt?.store.receiptLanguage ?? 'en';

  return (
    <div className="till app">
      <header className="till-head">
        <div className="who">
          <BrandMark size={18} />
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            <strong>{store.name}</strong>
            <span className="small" style={{ color: 'var(--night-muted)' }}>
              <span className="ltr">{tillCode}</span> · {user.name} · {t('shiftSince', { time: shiftTime })}
            </span>
          </div>
        </div>
        {online ? (
          <span className="pill pill-ok"><span className="led" />{t('online')}</span>
        ) : (
          <span className="pill pill-bad" role="status">{t('offline')}</span>
        )}
        <LanguageToggle value={lang} onChange={changeLanguage} onNight />
        <button type="button" className="head-btn" onClick={signOut}>{t('signOut')}</button>
      </header>

      <div className="till-body">
        <section className="bill" aria-label="Bill">
          <div className="field">
            <label className="label small" htmlFor="q">
              {t('searchLabel')} <kbd>F2</kbd>
            </label>
            <div className="searchbar">
              <SearchIcon />
              <input
                id="q"
                ref={searchRef}
                autoFocus
                autoComplete="off"
                value={query}
                placeholder={t('searchPlaceholder')}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    void onSearchEnter();
                  } else if (e.key === 'ArrowDown' && results) {
                    e.preventDefault();
                    setHighlight((h) => Math.min(h + 1, results.length - 1));
                  } else if (e.key === 'ArrowUp') {
                    e.preventDefault();
                    setHighlight((h) => Math.max(h - 1, 0));
                  }
                }}
              />
              <span className="tiny muted">{t('scannerReady')}</span>
            </div>
          </div>

          {results && (
            <div className="box" style={{ overflow: 'hidden' }}>
              <div className="results-head">{results.length ? t('matches', { n: results.length }) : t('noMatch')}</div>
              <div role="listbox" aria-label="Search results">
                {results.map((p, i) => (
                  <button key={p.id} type="button" role="option" aria-selected={i === highlight} className="result" onClick={() => add(p)}>
                    <span className="names">
                      <strong>{p.name}</strong>
                      <span className="small muted">
                        {[p.genericName, p.rack && `Rack ${p.rack}`].filter(Boolean).join(' · ')}
                      </span>
                    </span>
                    <span className="small" style={{ flex: '0 0 150px', color: p.stockBase > 0 ? 'var(--ink-2)' : 'var(--bad)' }}>
                      {t('inStock', { n: p.sellMode === 'weight' ? formatGrams(p.stockBase) : `${p.stockBase} ${p.baseUnit}` })}
                    </span>
                    <span className="num small" style={{ flex: '0 0 120px' }}>
                      {p.sellMode === 'weight'
                        ? `${formatRs(p.pricePerKg ?? 0)} ${t('perKg')}`
                        : p.units[0] && `${formatRs(p.units[0].price)} / ${p.units[0].name}`}
                    </span>
                  </button>
                ))}
              </div>
            </div>
          )}

          <div className="box">
            <div className="cart-scroll">
              <div className="cart">
                <div className="cart-row cart-head">
                  <span>{t('colItem')}</span>
                  <span>{t('colSellAs')}</span>
                  <span>{t('colQty')}</span>
                  <span className="end">{t('colPrice')}</span>
                  <span className="end">{t('colAmount')}</span>
                  <span />
                </div>
                {lines.length === 0 && <div className="empty">{t('emptyBill')}</div>}
                {lines.map((line) => {
                  const priced = priceLine(line);
                  const onBill = baseQtyOnBill(lines, line.product.id);
                  const tooMuch = onBill > line.product.stockBase;
                  const soon = monthsUntil(line.product.nextExpiry);
                  const meta = [
                    line.product.genericName,
                    line.product.nextBatchNo && line.product.nextBatchNo !== '-' && `${t('batch')} ${line.product.nextBatchNo}`,
                    line.product.nextExpiry && `${t('exp')} ${line.product.nextExpiry}`,
                  ]
                    .filter(Boolean)
                    .join(' · ');
                  return (
                    <div key={line.key}>
                      <div className="cart-row">
                        <div style={{ display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                            <span className="pname">{line.product.name}</span>
                            {line.product.rxRequired && <span className="chip chip-rx" title={t('rxTitle')}>{t('rx')}</span>}
                          </div>
                          <span className="tiny muted ltr" style={{ textAlign: 'start' }}>
                            {line.kind === 'weight'
                              ? `${line.product.genericName ?? ''} = ${line.mode === 'amount' ? formatGrams(priced.grams ?? 0) : formatRs(priced.total)}`
                              : meta}
                          </span>
                          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                            {soon !== null && soon <= 3 && <span className="chip chip-warn">{t('nearExpiry')}</span>}
                            {tooMuch && (
                              <span className="chip chip-bad">
                                {t('onlyInStock', {
                                  n: line.product.sellMode === 'weight' ? formatGrams(line.product.stockBase) : `${line.product.stockBase} ${line.product.baseUnit}`,
                                })}
                              </span>
                            )}
                          </div>
                        </div>
                        {line.kind === 'unit' ? (
                          <>
                            <div className="seg small on-soft" role="group" aria-label={t('colSellAs')}>
                              {line.product.units.map((u) => (
                                <button key={u.id} type="button" aria-pressed={u.id === line.unitId} onClick={() => update(line.key, { unitId: u.id })}>
                                  {lang === 'ur' && u.nameUr ? u.nameUr : u.name}
                                  {u.baseQty > 1 ? <span className="ltr">{` ×${u.baseQty}`}</span> : null}
                                </button>
                              ))}
                            </div>
                            <div className="qty">
                              <button type="button" aria-label="Decrease quantity" onClick={() => update(line.key, { qty: line.qty - 1 })}>−</button>
                              <input
                                aria-label="Quantity"
                                inputMode="numeric"
                                value={line.qty}
                                onChange={(e) => {
                                  const n = parseInt(e.target.value.replace(/\D/g, ''), 10);
                                  if (!Number.isNaN(n) && n > 0) update(line.key, { qty: Math.min(n, 9999) });
                                }}
                              />
                              <button type="button" aria-label="Increase quantity" onClick={() => update(line.key, { qty: line.qty + 1 })}>+</button>
                            </div>
                          </>
                        ) : (
                          <WeightEditor line={line} onChange={(patch) => update(line.key, patch)} />
                        )}
                        <span className="end num small muted">
                          {line.kind === 'weight' ? `${formatRs(priced.unitPrice)} ${t('perKg')}` : formatRs(priced.unitPrice)}
                        </span>
                        <span className="end num" style={{ fontWeight: 600 }}>{formatRs(priced.total)}</span>
                        <button type="button" className="icon-btn" aria-label={t('removeItem')} onClick={() => remove(line.key)}>
                          <TrashIcon />
                        </button>
                      </div>
                      {line.kind === 'weight' && <QuickWeights onPick={(patch) => update(line.key, patch)} />}
                    </div>
                  );
                })}
              </div>
            </div>
          </div>
        </section>

        <aside className="side" aria-label={t('paymentMethod')}>
          <div className="box box-pad">
            <label className="label small" htmlFor="cust">{t('customer')}</label>
            {!addingCustomer ? (
              <select
                id="cust"
                className="select"
                value={customerId}
                onChange={(e) => {
                  if (e.target.value === '__new') setAddingCustomer(true);
                  else setCustomerId(e.target.value);
                }}
              >
                <option value="">{t('walkIn')}</option>
                {customers.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                    {c.phone ? ` (${c.phone})` : ''}
                    {c.balance ? ` · ${t('khataBalance', { amount: formatRs(c.balance) })}` : ''}
                  </option>
                ))}
                <option value="__new">{t('newCustomer')}</option>
              </select>
            ) : (
              <NewCustomerForm
                onCancel={() => setAddingCustomer(false)}
                onCreated={(c) => {
                  setCustomers((list) => [...list, c]);
                  setCustomerId(c.id);
                  setAddingCustomer(false);
                }}
              />
            )}
          </div>

          <div className="box box-pad">
            <div className="row"><span className="muted">{t('items')}</span><span className="num">{totals.itemCount}</span></div>
            <div className="row"><span className="muted">{t('subtotal')}</span><span className="num">{formatRs(totals.subtotal)}</span></div>
            <div className="row" style={{ flexWrap: 'wrap' }}>
              <span className="muted">{t('discount')}</span>
              <div className="seg small" role="group" aria-label={t('discount')}>
                {DISCOUNTS.map((d) => (
                  <button key={d} type="button" aria-pressed={discountBp === d} onClick={() => setDiscountBp(d)}>
                    {d === 0 ? t('none') : <span className="ltr">{d / 100}%</span>}
                  </button>
                ))}
              </div>
            </div>
            {totals.discount > 0 && (
              <div className="row"><span className="muted">{t('discountAmount')}</span><span className="num" style={{ color: 'var(--bad)' }}>− {formatRs(totals.discount)}</span></div>
            )}
            {totals.tax > 0 && (
              <div className="row small"><span className="muted">{t('taxIncluded')}</span><span className="num muted">{formatRs(totals.tax)}</span></div>
            )}
            <div className="total-row">
              <strong style={{ fontSize: 18 }}>{t('total')}</strong>
              <span className="total-amount">{formatRs(totals.total)}</span>
            </div>
          </div>

          <div className="box box-pad">
            <span className="label small">{t('paymentMethod')}</span>
            <div className="methods">
              {METHODS.map((m) => (
                <button key={m} type="button" className="choice" aria-pressed={method === m} onClick={() => setMethod(m)}>
                  {t(m)}
                </button>
              ))}
            </div>
            {method === 'cash' && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                <label className="label small" htmlFor="tendered">{t('cashReceived')}</label>
                <input
                  id="tendered"
                  className="input big ltr"
                  inputMode="decimal"
                  placeholder={formatRs(totals.total)}
                  value={tendered}
                  onChange={(e) => setTendered(e.target.value)}
                />
                <div className="tenders">
                  <button type="button" onClick={() => setTendered('')}>{t('exact')}</button>
                  {[1_000, 2_000, 5_000].map((n) => (
                    <button key={n} type="button" onClick={() => setTendered(String(n))}>{n.toLocaleString('en-US')}</button>
                  ))}
                </div>
                <div className="change-box">
                  <span className="small muted">{t('received')} <span className="num">{tenderedPaisa === null ? '—' : formatRs(tenderedPaisa)}</span></span>
                  <strong>
                    {t('change')}{' '}
                    <span className="num" style={{ color: change !== null && change < 0 ? 'var(--bad)' : 'var(--brand)' }}>
                      {change === null ? '—' : change < 0 ? t('shortBy', { amount: formatRs(-change) }) : formatRs(change)}
                    </span>
                  </strong>
                </div>
              </div>
            )}
            {method === 'khata' && (
              <div className={`alert ${customer ? 'alert-ok' : 'alert-warn'}`}>
                {customer
                  ? `${customer.name} · ${t('khataBalance', { amount: formatRs(customer.balance) })}`
                  : t('khataNeedsCustomer')}
              </div>
            )}
          </div>

          {error && <div className="alert alert-bad" role="alert">{error}</div>}

          <div className="grid-2" style={{ gridTemplateColumns: '1fr' }}>
            <button type="button" className="btn btn-danger" onClick={resetBill} disabled={lines.length === 0 || busy}>
              {t('cancelBill')}
            </button>
          </div>
          <button type="button" className="btn btn-primary pay-btn" onClick={() => void pay()} disabled={lines.length === 0 || busy}>
            {busy ? t('saving') : t('pay')} <kbd>F12</kbd>
          </button>
        </aside>
      </div>

      <footer className="keys">
        <span><kbd>F2</kbd> {t('keySearch')}</span>
        <span><kbd>F12</kbd> {t('keyPay')}</span>
        <span><kbd>Esc</kbd> {t('keyClear')}</span>
      </footer>

      {approvalNeeded && (
        <ApprovalDialog
          message={approvalNeeded}
          busy={busy}
          onCancel={() => setApprovalNeeded(null)}
          onApprove={(userId, secret) => void pay({ userId, secret })}
          error={error}
        />
      )}

      {receipt && (
        <>
          <div className="overlay" role="dialog" aria-modal="true" aria-label={t('saleDone')}>
            <div className="dialog receipt-dialog">
              <div className="receipt-preview">
                <Receipt80 receipt={receipt} language={receiptLanguage} />
              </div>
              <div className="actions">
                <h2>{t('saleDone')}</h2>
                <span className="mono" style={{ fontSize: 13 }}>{receipt.receiptNo}</span>
                {receipt.changeDue > 0 && (
                  <div className="alert alert-ok" style={{ fontSize: 22, fontWeight: 700 }}>
                    {t('changeToGive', { amount: formatRs(receipt.changeDue) })}
                  </div>
                )}
                <button type="button" className="btn" onClick={() => window.print()}>{t('print')}</button>
                <button type="button" className="btn btn-primary btn-lg" onClick={resetBill} autoFocus>
                  {t('newSale')} <kbd>Esc</kbd>
                </button>
              </div>
            </div>
          </div>
          {createPortal(
            <div className="print-root">
              <Receipt80 receipt={receipt} language={receiptLanguage} />
            </div>,
            document.body,
          )}
        </>
      )}
    </div>
  );
}

function WeightEditor({ line, onChange }: { line: WeightLine; onChange: (patch: Partial<WeightLine>) => void }) {
  const { t } = useI18n();
  const priced = priceLine(line);
  const switchMode = (mode: 'amount' | 'grams') => {
    if (mode === line.mode) return;
    // Keep the same quantity when switching: Rs 100 of sugar stays 625 g.
    onChange(mode === 'amount' ? { mode, value: priced.total } : { mode, value: priced.grams ?? 0 });
  };
  const shown = line.mode === 'amount' ? String(Math.round(line.value / 100)) : String(line.value);
  return (
    <>
      <div className="seg small on-soft" role="group" aria-label={t('colSellAs')}>
        <button type="button" aria-pressed={line.mode === 'amount'} onClick={() => switchMode('amount')}>{t('byRs')}</button>
        <button type="button" aria-pressed={line.mode === 'grams'} onClick={() => switchMode('grams')}>{t('byWeight')}</button>
      </div>
      <div className="weight-input">
        {line.mode === 'amount' && <span className="small muted">Rs</span>}
        <input
          aria-label={line.mode === 'amount' ? 'Amount in rupees' : 'Weight in grams'}
          inputMode="numeric"
          value={shown}
          onChange={(e) => {
            const n = parseInt(e.target.value.replace(/\D/g, ''), 10);
            const v = Number.isNaN(n) ? 0 : n;
            onChange({ value: line.mode === 'amount' ? v * 100 : v });
          }}
        />
        {line.mode === 'grams' && <span className="small muted">g</span>}
      </div>
    </>
  );
}

function QuickWeights({ onPick }: { onPick: (patch: Partial<WeightLine>) => void }) {
  const { t } = useI18n();
  const picks: [string, 'amount' | 'grams', number][] = [
    ['Rs 10', 'amount', 1_000],
    ['Rs 20', 'amount', 2_000],
    ['Rs 50', 'amount', 5_000],
    ['Rs 100', 'amount', 10_000],
    ['250 g', 'grams', 250],
    ['500 g', 'grams', 500],
    ['1 kg', 'grams', 1_000],
  ];
  return (
    <div className="quick-row">
      <span className="tiny muted">{t('quick')}</span>
      {picks.map(([label, mode, value]) => (
        <button key={label} type="button" onClick={() => onPick({ mode, value })}>{label}</button>
      ))}
    </div>
  );
}

function NewCustomerForm({ onCreated, onCancel }: { onCreated: (c: Customer) => void; onCancel: () => void }) {
  const { t } = useI18n();
  const [name, setName] = useState('');
  const [phone, setPhone] = useState('');
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      onCreated(await api.createCustomer({ name, phone: phone || undefined }));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : String(err));
    }
  };
  return (
    <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <input className="input" aria-label={t('newCustomerName')} placeholder={t('newCustomerName')} value={name} onChange={(e) => setName(e.target.value)} autoFocus required />
      <input className="input ltr" aria-label={t('newCustomerPhone')} placeholder={t('newCustomerPhone')} value={phone} onChange={(e) => setPhone(e.target.value)} inputMode="tel" />
      {error && <div className="alert alert-bad">{error}</div>}
      <div style={{ display: 'flex', gap: 8 }}>
        <button type="button" className="btn" onClick={onCancel}>{t('cancel')}</button>
        <button type="submit" className="btn btn-primary" style={{ flex: 1 }}>{t('save')}</button>
      </div>
    </form>
  );
}

function ApprovalDialog({
  message,
  busy,
  error,
  onApprove,
  onCancel,
}: {
  message: string;
  busy: boolean;
  error: string | null;
  onApprove: (userId: string, secret: string) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [approvers, setApprovers] = useState<LoginTile[]>([]);
  const [who, setWho] = useState('');
  const [secret, setSecret] = useState('');
  useEffect(() => {
    api.loginTiles().then((list) => {
      const can = list.filter((u) => u.canApprove);
      setApprovers(can);
      if (can[0]) setWho(can[0].id);
    });
  }, []);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (who && secret) onApprove(who, secret);
  };
  return (
    <div className="overlay" role="dialog" aria-modal="true" aria-labelledby="approval-title">
      <form className="dialog" onSubmit={submit}>
        <h2 id="approval-title">{t('approvalTitle')}</h2>
        <div className="alert alert-warn">{message}</div>
        <div className="field">
          <span className="label">{t('approvalWho')}</span>
          <div className="tiles">
            {approvers.map((a) => (
              <button key={a.id} type="button" className="tile" aria-pressed={who === a.id} onClick={() => setWho(a.id)}>
                <span className="tile-name">{a.name}</span>
              </button>
            ))}
          </div>
        </div>
        <div className="field">
          <label className="label" htmlFor="approval-secret">{t('approvalSecret')}</label>
          <input id="approval-secret" type="password" className="input ltr" value={secret} onChange={(e) => setSecret(e.target.value)} autoFocus autoComplete="off" />
        </div>
        {error && <div className="alert alert-bad">{error}</div>}
        <div style={{ display: 'flex', gap: 8 }}>
          <button type="button" className="btn" onClick={onCancel}>{t('cancel')}</button>
          <button type="submit" className="btn btn-primary" style={{ flex: 1 }} disabled={busy || !secret}>{t('approve')}</button>
        </div>
      </form>
    </div>
  );
}
