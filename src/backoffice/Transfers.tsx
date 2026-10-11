import { useEffect, useRef, useState } from 'react';
import { api } from '../api/client';
import type { BatchDetail, Product, ProductDetail, StoreHeader, TransferNote, TransferRow, TransferView } from '../api/types';
import { SearchIcon, TrashIcon } from '../components/common';
import { formatMonth } from '../domain/dates';
import { ltr } from '../domain/money';
import { countingUnit, formatStock } from '../domain/stock';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import { PrintPortal } from '../print/DocView';
import { newClientRef } from './parts';
import { formatDateTime, transferDoc, type Doc } from '../print/doc';
import { printDoc, type PrinterSettings } from '../print/printer';
import type { Page } from './Backoffice';

type View = { kind: 'home' } | { kind: 'send' } | { kind: 'receive'; note: TransferNote } | { kind: 'show'; transfer: TransferView };

const RECEIVE_REASONS: [string, StringKey][] = [
  ['broken', 'rsnBroken'],
  ['missing', 'rsnMissing'],
  ['short_sent', 'rsnLess'],
];

function downloadNote(note: TransferNote) {
  const blob = new Blob([JSON.stringify(note, null, 1)], { type: 'application/json' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `${note.transferNo}.transfer.json`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

export function Transfers({ store, printer, go }: { store: StoreHeader; printer: PrinterSettings; go: (p: Page) => void }) {
  const { t, lang } = useI18n();
  const [view, setView] = useState<View>({ kind: 'home' });
  const [list, setList] = useState<TransferRow[] | null>(null);
  const [stores, setStores] = useState<{ id: string; code: string; name: string }[]>([]);
  const [incoming, setIncoming] = useState<TransferNote[] | null>(null);
  const [cloudErrors, setCloudErrors] = useState<[string, string][]>([]);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [printing, setPrinting] = useState<Doc | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const load = () => {
    api
      .transfers()
      .then((r) => {
        setList(r.transfers);
        setStores(r.stores);
      })
      .catch((e) => setError(describeError(e, lang)));
    api
      .incomingTransfers()
      .then((r) => {
        setIncoming(r.notes);
        setCloudErrors(r.errors);
      })
      .catch(() => setIncoming([]));
  };
  useEffect(load, [lang]);

  const openFile = async (file: File) => {
    setError(null);
    try {
      const note = JSON.parse(await file.text()) as TransferNote;
      if (note.kind !== 'medstore-transfer' || !Array.isArray(note.lines)) throw new Error('bad');
      setView({ kind: 'receive', note });
    } catch {
      setError(t('trBadFile'));
    } finally {
      if (fileInput.current) fileInput.current.value = '';
    }
  };

  const show = async (id: string) => {
    try {
      setView({ kind: 'show', transfer: await api.transfer(id) });
    } catch (e) {
      setError(describeError(e, lang));
    }
  };

  const print = async (tr: TransferView) => {
    const doc = transferDoc(tr, store, store.receiptLanguage);
    setPrinting(doc);
    try {
      await printDoc(doc, printer);
    } catch (e) {
      setError(t('printFailed', { msg: String(e) }));
    }
  };

  const header = (
    <div className="bo-head">
      <div>
        <span className="small">
          <button type="button" className="link-btn" onClick={() => go({ name: 'inventory' })}>
            {t('navInventory')}
          </button>{' '}
          ›{' '}
          <button type="button" className="link-btn" onClick={() => setView({ kind: 'home' })}>
            {t('branchTransfers')}
          </button>
        </span>
        <h1>{t('trTitle')}</h1>
      </div>
      {view.kind === 'home' && (
        <button type="button" className="btn btn-primary" onClick={() => setView({ kind: 'send' })}>
          {t('trNew')}
        </button>
      )}
    </div>
  );

  let body;
  if (view.kind === 'send') {
    body = (
      <SendForm
        stores={stores}
        onSent={(tr) => {
          setMessage(t('trSent', { no: tr.transferNo }));
          setView({ kind: 'show', transfer: tr });
          load();
        }}
      />
    );
  } else if (view.kind === 'receive') {
    body = (
      <ReceiveForm
        note={view.note}
        onReceived={(tr) => {
          setMessage(t('trReceivedOk', { no: tr.transferNo }));
          setView({ kind: 'show', transfer: tr });
          load();
        }}
      />
    );
  } else if (view.kind === 'show') {
    const tr = view.transfer;
    body = (
      <section className="card">
        <div className="card-head">
          <div>
            <h2 className="mono">{tr.transferNo}</h2>
            <span className="small muted">
              {tr.direction === 'out' ? t('trDirOut', { code: `${tr.otherName ?? ''} ${tr.otherCode}`.trim() }) : t('trDirIn', { code: `${tr.otherName ?? ''} ${tr.otherCode}`.trim() })} ·{' '}
              {tr.userName} · <span className="ltr">{formatDateTime(tr.createdAt)}</span>
              {tr.note ? ` · ${tr.note}` : ''}
            </span>
          </div>
          <div className="bo-actions">
            <button type="button" className="btn" onClick={() => void print(tr)}>
              {t('trPrint')}
            </button>
            {tr.payload && (
              <button type="button" className="btn btn-primary" onClick={() => downloadNote(tr.payload!)}>
                {t('trDownload')}
              </button>
            )}
          </div>
        </div>
        {tr.payload && <span className="small muted">{t('trFileHelp')}</span>}
        <div className="tbl-scroll">
          <table className="tbl">
            <thead>
              <tr>
                <th>{t('colProduct')}</th>
                <th>{t('colBatch')}</th>
                <th className="num">{t('trColSent')}</th>
                {tr.direction === 'in' && <th className="num">{t('trColReceived')}</th>}
                {tr.direction === 'in' && <th>{t('colReason')}</th>}
              </tr>
            </thead>
            <tbody>
              {tr.lines.map((l, i) => (
                <tr key={i}>
                  <td className="pname-cell">{l.productName}</td>
                  <td className="small ltr">
                    {l.batchNo}
                    {l.expiry ? ` · ${formatMonth(l.expiry)}` : ''}
                  </td>
                  <td className="num">
                    {l.sentQty.toLocaleString('en-US')} {l.baseUnit}
                  </td>
                  {tr.direction === 'in' && (
                    <td className="num" style={{ color: (l.receivedQty ?? 0) < l.sentQty ? 'var(--bad)' : undefined }}>
                      {(l.receivedQty ?? 0).toLocaleString('en-US')} {l.baseUnit}
                    </td>
                  )}
                  {tr.direction === 'in' && (
                    <td className="small">{l.reason ? t((RECEIVE_REASONS.find(([r]) => r === l.reason)?.[1] ?? 'adjOther') as StringKey) : ''}</td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    );
  } else {
    body = (
      <div className="cols">
        <section className="card grow">
          <h2>{t('trIncoming')}</h2>
          {incoming === null && <span className="muted">{t('loading')}</span>}
          {incoming?.length === 0 && <span className="small muted">{t('trIncomingNone')}</span>}
          {cloudErrors.map(([name, msg]) => (
            <div key={name} className="alert alert-warn small">
              {t('trCloudError', { name, msg })}
            </div>
          ))}
          {incoming?.map((n) => (
            <div key={n.transferId} className="list-row">
              <div className="main">
                <strong className="mono">{n.transferNo}</strong>
                <span className="small muted">
                  {t('trFrom', { name: n.fromName })} · {t('trSentBy', { name: n.sentBy, date: formatDateTime(n.sentAt) })} ·{' '}
                  {n.lines.length}
                </span>
              </div>
              <button type="button" className="btn btn-primary" style={{ height: 38 }} onClick={() => setView({ kind: 'receive', note: n })}>
                {t('trReceive')}
              </button>
            </div>
          ))}
          <div>
            <input ref={fileInput} id="tr-file" type="file" accept=".json,application/json" className="visually-hidden" onChange={(e) => e.target.files?.[0] && void openFile(e.target.files[0])} />
            <label htmlFor="tr-file" className="btn" style={{ cursor: 'pointer' }}>
              {t('trOpenFile')}
            </label>
          </div>
        </section>
        <section className="card aside">
          <h2>{t('branchTransfers')}</h2>
          {list?.length === 0 && <span className="small muted">{t('trNoTransfers')}</span>}
          {list?.map((r) => (
            <button
              key={r.id}
              type="button"
              className="list-row"
              style={{ border: 0, borderBottom: '1px solid var(--line-2)', background: 'transparent', width: '100%', textAlign: 'start' }}
              onClick={() => void show(r.id)}
            >
              <div className="main">
                <strong className="mono">{r.transferNo}</strong>
                <span className="small muted">
                  {r.direction === 'out' ? t('trDirOut', { code: r.otherName ?? r.otherCode }) : t('trDirIn', { code: r.otherName ?? r.otherCode })} ·{' '}
                  <span className="ltr">{formatDateTime(r.createdAt)}</span>
                </span>
              </div>
              {r.differences > 0 ? (
                <span className="tag tag-warn">{t('trDiffs', { n: r.differences })}</span>
              ) : (
                <span className={`tag ${r.direction === 'in' ? 'tag-ok' : 'tag-mute'}`}>{r.direction === 'in' ? t('trReceived') : t('trColSent')}</span>
              )}
            </button>
          ))}
        </section>
      </div>
    );
  }

  return (
    <>
      {header}
      {message && view.kind === 'show' && <div className="alert alert-ok" role="status">{message}</div>}
      {error && <div className="alert alert-bad" role="alert">{error}</div>}
      {body}
      <PrintPortal doc={printing} />
    </>
  );
}

interface SendLineState {
  key: string;
  product: ProductDetail;
  batch: BatchDetail;
  packs: string;
  loose: string;
}

function SendForm({ stores, onSent }: { stores: { id: string; code: string; name: string }[]; onSent: (tr: TransferView) => void }) {
  const { t, lang } = useI18n();
  const [to, setTo] = useState(stores[0]?.code ?? '__other');
  const [code, setCode] = useState('');
  const [name, setName] = useState('');
  const [note, setNote] = useState('');
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<Product[]>([]);
  const [lines, setLines] = useState<SendLineState[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** One reference per transfer being written, so a retry can't send the stock twice. */
  const [clientRef] = useState(newClientRef);

  useEffect(() => {
    const q = query.trim();
    if (!q) return setResults([]);
    const timer = setTimeout(() => api.searchProducts(q).then(setResults).catch(() => setResults([])), 200);
    return () => clearTimeout(timer);
  }, [query]);

  const add = async (p: Product) => {
    setQuery('');
    setResults([]);
    try {
      const d = await api.productDetail(p.id);
      // Earliest-expiring stock goes first, as at the till.
      const batches = d.batches.filter((b) => b.qtyOnHand > 0 && !b.expired);
      if (!batches.length) return setError(t('noBatches'));
      setLines((ls) => [...ls, { key: `${d.id}-${Date.now()}`, product: d, batch: batches[0], packs: '', loose: '' }]);
    } catch (e) {
      setError(describeError(e, lang));
    }
  };

  const baseQty = (l: SendLineState) => {
    if (l.product.sellMode === 'weight') return Math.round(Number(l.packs || 0) * 1000);
    const pack = countingUnit(l.product.units);
    return Number(l.packs || 0) * (pack?.baseQty ?? 1) + Number(l.loose || 0);
  };

  const send = async () => {
    setError(null);
    const toCode = to === '__other' ? code.trim() : to;
    const toName = to === '__other' ? name.trim() : stores.find((s) => s.code === to)?.name;
    setBusy(true);
    try {
      onSent(
        await api.sendTransfer({
          toCode,
          toName: toName || undefined,
          note: note.trim() || undefined,
          lines: lines.map((l) => ({ batchId: l.batch.id, qty: baseQty(l) })),
          clientRef,
        }),
      );
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  return (
    <section className="card">
      <span className="small muted">{t('trHelp')}</span>
      <div className="form-grid">
        <div className="field">
          <label className="label" htmlFor="tr-to">{t('trTo')}</label>
          <select id="tr-to" className="select" value={to} onChange={(e) => setTo(e.target.value)}>
            {stores.map((s) => (
              <option key={s.code} value={s.code}>
                {s.name} ({s.code})
              </option>
            ))}
            <option value="__other">{t('trOtherBranch')}</option>
          </select>
        </div>
        {to === '__other' && (
          <>
            <div className="field">
              <label className="label" htmlFor="tr-code">{t('trToCode')}</label>
              <input id="tr-code" className="input ltr" placeholder="B2" maxLength={6} value={code} onChange={(e) => setCode(e.target.value.toUpperCase())} />
            </div>
            <div className="field">
              <label className="label" htmlFor="tr-name">{t('trToName')}</label>
              <input id="tr-name" className="input" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
          </>
        )}
        <div className="field">
          <label className="label" htmlFor="tr-note">{t('trCarrier')}</label>
          <input id="tr-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
        </div>
      </div>
      <div className="searchbar" style={{ height: 48 }}>
        <SearchIcon />
        <label className="visually-hidden" htmlFor="tr-q">{t('trAddItem')}</label>
        <input id="tr-q" placeholder={t('trAddItem')} value={query} autoComplete="off" onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && results[0] && void add(results[0])} />
      </div>
      {results.length > 0 && (
        <div className="result-list">
          {results.map((p) => (
            <button key={p.id} type="button" className="result" onClick={() => void add(p)}>
              <span className="names">
                <span className="pname">{p.name}</span>
              </span>
            </button>
          ))}
        </div>
      )}
      {lines.length > 0 && (
        <div className="tbl-scroll">
          <table className="tbl" style={{ minWidth: 760 }}>
            <thead>
              <tr>
                <th>{t('colProduct')}</th>
                <th>{t('colBatch')}</th>
                <th>{t('trQty')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {lines.map((l) => {
                const pack = l.product.sellMode === 'weight' ? null : countingUnit(l.product.units);
                const set = (p: Partial<SendLineState>) => setLines(lines.map((x) => (x.key === l.key ? { ...x, ...p } : x)));
                const qty = baseQty(l);
                const tooMuch = qty > l.batch.qtyOnHand;
                return (
                  <tr key={l.key}>
                    <td>
                      <span className="pname" style={{ fontSize: 14 }}>{l.product.name}</span>
                      {l.product.coldChain && <span className="tag tag-warn" style={{ marginInlineStart: 6 }}>{t('trKeepCold')}</span>}
                    </td>
                    <td>
                      <select
                        aria-label={t('colBatch')}
                        value={l.batch.id}
                        onChange={(e) => set({ batch: l.product.batches.find((b) => b.id === e.target.value)! })}
                      >
                        {l.product.batches
                          .filter((b) => b.qtyOnHand > 0 && !b.expired)
                          .map((b) => (
                            <option key={b.id} value={b.id}>
                              {b.batchNo}
                              {b.expiry ? ` · ${formatMonth(b.expiry)}` : ''} · {ltr(formatStock(b.qtyOnHand, l.product.sellMode, l.product.baseUnit, l.product.units))}
                            </option>
                          ))}
                      </select>
                    </td>
                    <td style={{ width: 230 }}>
                      <div style={{ display: 'flex', gap: 6, alignItems: 'center', direction: 'ltr' }}>
                        <input aria-label={t('trQty')} className={`n${tooMuch ? ' invalid' : ''}`} style={{ width: 70 }} inputMode="decimal" value={l.packs} onChange={(e) => set({ packs: e.target.value })} />
                        <span className="tiny muted">{l.product.sellMode === 'weight' ? 'kg' : pack?.name ?? l.product.baseUnit}</span>
                        {pack && (
                          <>
                            <input aria-label={l.product.baseUnit} className={`n${tooMuch ? ' invalid' : ''}`} style={{ width: 56 }} inputMode="numeric" value={l.loose} onChange={(e) => set({ loose: e.target.value })} />
                            <span className="tiny muted">{l.product.baseUnit}</span>
                          </>
                        )}
                      </div>
                    </td>
                    <td style={{ width: 48 }}>
                      <button type="button" className="icon-btn" aria-label={t('remove')} onClick={() => setLines(lines.filter((x) => x.key !== l.key))}>
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
      {error && <div className="alert alert-bad" role="alert">{error}</div>}
      <button
        type="button"
        className="btn btn-primary btn-lg"
        style={{ alignSelf: 'flex-start' }}
        disabled={busy || !lines.length || lines.some((l) => baseQty(l) <= 0 || baseQty(l) > l.batch.qtyOnHand)}
        onClick={() => void send()}
      >
        {t('trSend')}
      </button>
    </section>
  );
}

function ReceiveForm({ note, onReceived }: { note: TransferNote; onReceived: (tr: TransferView) => void }) {
  const { t, lang } = useI18n();
  const [received, setReceived] = useState(note.lines.map((l) => ({ qty: String(l.qty), reason: '' })));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      onReceived(
        await api.receiveTransfer(
          note,
          received.map((r) => ({ qty: Math.round(Number(r.qty) || 0), reason: r.reason || undefined })),
        ),
      );
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  return (
    <section className="card">
      <div>
        <h2 className="mono">{note.transferNo}</h2>
        <span className="small muted">
          {t('trFrom', { name: `${note.fromName} (${note.fromCode})` })} · {t('trSentBy', { name: note.sentBy, date: formatDateTime(note.sentAt) })}
          {note.note ? ` · ${note.note}` : ''}
        </span>
      </div>
      <div className="tbl-scroll">
        <table className="tbl" style={{ minWidth: 720 }}>
          <thead>
            <tr>
              <th>{t('colProduct')}</th>
              <th>{t('colBatch')}</th>
              <th className="num">{t('trColSent')}</th>
              <th>{t('trColReceived')}</th>
              <th>{t('colReason')}</th>
            </tr>
          </thead>
          <tbody>
            {note.lines.map((l, i) => {
              const r = received[i];
              const short = Number(r.qty) < l.qty;
              const set = (p: Partial<typeof r>) => setReceived(received.map((x, j) => (j === i ? { ...x, ...p } : x)));
              return (
                <tr key={i}>
                  <td>
                    <span className="pname" style={{ fontSize: 14 }}>{l.productName}</span>
                    {l.coldChain && <span className="tag tag-warn" style={{ marginInlineStart: 6 }}>{t('trKeepCold')}</span>}
                  </td>
                  <td className="small ltr">
                    {l.batchNo}
                    {l.expiry ? ` · ${formatMonth(l.expiry)}` : ''}
                  </td>
                  <td className="num">
                    {l.qty.toLocaleString('en-US')} {l.baseUnit}
                  </td>
                  <td style={{ width: 150 }}>
                    <div style={{ display: 'flex', gap: 6, alignItems: 'center', direction: 'ltr' }}>
                      <input aria-label={`${t('trColReceived')} ${l.productName}`} className="n" style={{ width: 90 }} inputMode="numeric" value={r.qty} onChange={(e) => set({ qty: e.target.value.replace(/\D/g, '') })} />
                      <span className="tiny muted">{l.baseUnit}</span>
                    </div>
                  </td>
                  <td style={{ width: 200 }}>
                    {short && (
                      <select aria-label={t('colReason')} value={r.reason} onChange={(e) => set({ reason: e.target.value })}>
                        <option value="">—</option>
                        {RECEIVE_REASONS.map(([v, k]) => (
                          <option key={v} value={v}>
                            {t(k)}
                          </option>
                        ))}
                      </select>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {error && <div className="alert alert-bad" role="alert">{error}</div>}
      <button type="button" className="btn btn-primary btn-lg" style={{ alignSelf: 'flex-start' }} disabled={busy} onClick={() => void save()}>
        {t('trReceive')}
      </button>
    </section>
  );
}
