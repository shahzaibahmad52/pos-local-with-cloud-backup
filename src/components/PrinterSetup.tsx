import { useEffect, useState } from 'react';
import type { StoreHeader } from '../api/types';
import { useI18n } from '../i18n/i18n';
import { isDesktop } from '../platform';
import { DocView, PrintPortal } from '../print/DocView';
import type { Doc } from '../print/doc';
import { canPrintDirect, listPrinters, openDrawer, printDoc, savePrinterSettings, type PrinterSettings } from '../print/printer';

function testDoc(store: StoreHeader, tillCode: string): Doc {
  return {
    dir: 'ltr',
    urdu: true,
    blocks: [
      { kind: 'title', text: store.name },
      { kind: 'center', text: 'Printer test · پرنٹر ٹیسٹ', bold: true },
      { kind: 'rule' },
      { kind: 'row', label: 'Till · ٹِل', value: tillCode },
      { kind: 'row', label: 'Total · کل رقم', value: '1,234', bold: true, big: true },
      { kind: 'rule', heavy: true },
      { kind: 'center', text: 'شکریہ! اللہ آپ کو صحت دے۔' },
    ],
  };
}

export function PrinterSetup({
  settings,
  store,
  tillCode,
  onSaved,
  onClose,
}: {
  settings: PrinterSettings;
  store: StoreHeader;
  tillCode: string;
  onSaved: (s: PrinterSettings) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(settings);
  const [printers, setPrinters] = useState<string[]>([]);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const doc = testDoc(store, tillCode);

  useEffect(() => {
    listPrinters().then(setPrinters).catch((e) => setMessage({ ok: false, text: String(e) }));
  }, []);

  const set = <K extends keyof PrinterSettings>(k: K, v: PrinterSettings[K]) => setDraft({ ...draft, [k]: v });
  const run = async (fn: () => Promise<void>, ok: string) => {
    setMessage(null);
    try {
      await fn();
      setMessage({ ok: true, text: ok });
    } catch (e) {
      setMessage({ ok: false, text: t('printFailed', { msg: String(e) }) });
    }
  };

  return (
    <div className="overlay" role="dialog" aria-modal="true" aria-labelledby="printer-title">
      <div className="dialog" style={{ maxWidth: 760, flexDirection: 'row', flexWrap: 'wrap', alignItems: 'flex-start' }}>
        <div style={{ flex: '1 1 320px', display: 'flex', flexDirection: 'column', gap: 12 }}>
          <h2 id="printer-title">{t('printerTitle')}</h2>
          <span className="small muted">{t('printerHelp')}</span>
          {!isDesktop() && <div className="alert alert-warn small">{t('printerBrowserNote')}</div>}
          {isDesktop() && (
            <div className="field">
              <label className="label" htmlFor="printer-name">{t('printerName')}</label>
              <select id="printer-name" className="select" value={draft.name ?? ''} onChange={(e) => set('name', e.target.value || null)}>
                <option value="">{t('printerNone')}</option>
                {printers.map((p) => (
                  <option key={p} value={p}>{p}</option>
                ))}
              </select>
            </div>
          )}
          <div className="field">
            <span className="label">{t('paperWidth')}</span>
            <div className="seg" role="group" aria-label={t('paperWidth')}>
              <button type="button" aria-pressed={draft.widthDots === 576} onClick={() => set('widthDots', 576)}>80 mm</button>
              <button type="button" aria-pressed={draft.widthDots === 384} onClick={() => set('widthDots', 384)}>58 mm</button>
            </div>
          </div>
          <label className="row" style={{ justifyContent: 'flex-start', gap: 10, minHeight: 36 }}>
            <input type="checkbox" checked={draft.autoPrint} onChange={(e) => set('autoPrint', e.target.checked)} />
            {t('autoPrint')}
          </label>
          {isDesktop() && (
            <>
              <label className="row" style={{ justifyContent: 'flex-start', gap: 10, minHeight: 36 }}>
                <input type="checkbox" checked={draft.openDrawerOnCash} onChange={(e) => set('openDrawerOnCash', e.target.checked)} />
                {t('drawerOnCash')}
              </label>
              <label className="row" style={{ justifyContent: 'flex-start', gap: 10, minHeight: 36 }}>
                <input type="checkbox" checked={draft.cut} onChange={(e) => set('cut', e.target.checked)} />
                {t('cutPaper')}
              </label>
            </>
          )}
          {message && <div className={`alert ${message.ok ? 'alert-ok' : 'alert-bad'}`} role="status">{message.text}</div>}
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
            <button type="button" className="btn" onClick={() => void run(() => printDoc(doc, draft), t('saved'))}>{t('testPrint')}</button>
            {canPrintDirect(draft) && (
              <button type="button" className="btn" onClick={() => void run(() => openDrawer(draft), t('drawerOpened'))}>{t('testDrawer')}</button>
            )}
          </div>
          <div style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={onClose}>{t('close')}</button>
            <button
              type="button"
              className="btn btn-primary"
              style={{ flex: 1 }}
              onClick={() =>
                void run(async () => {
                  await savePrinterSettings(draft);
                  onSaved(draft);
                }, t('saved'))
              }
            >
              {t('save')}
            </button>
          </div>
        </div>
        <div className="receipt-preview">
          <DocView doc={doc} />
        </div>
      </div>
      <PrintPortal doc={doc} />
    </div>
  );
}
