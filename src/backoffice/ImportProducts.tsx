import { useRef, useState } from 'react';
import { api } from '../api/client';
import type { ImportReport, ImportRow } from '../api/types';
import { COLUMNS, EXAMPLES, readSheet, type Cell } from '../domain/importSheet';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { Page } from './Backoffice';
import { saveFile } from '../platform';

interface Loaded {
  file: string;
  rows: ImportRow[];
  /** Raw sheet rows by sheet row number, for the "rows to fix" download. */
  raw: Map<number, Cell[]>;
  header: Cell[];
  clientProblems: { row: number; name: string; message: string }[];
  missing: string[];
  report: ImportReport | null;
}

export function ImportProducts({ go }: { go: (p: Page) => void }) {
  const { t, lang } = useI18n();
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const downloadTemplate = async () => {
    const { default: writeXlsxFile } = await import('write-excel-file/browser');
    const header = COLUMNS.map((c) => ({ value: c.title, fontWeight: 'bold' as const, backgroundColor: '#E3F2FB' }));
    const blob = await writeXlsxFile([header, ...EXAMPLES], {
      columns: COLUMNS.map((c) => ({ width: c.width })),
      stickyRowsCount: 1,
    }).toBlob();
    const path = await saveFile('products-template.xlsx', blob);
    if (path) setSaved(t('savedTo', { file: path }));
  };

  const choose = async (file: File) => {
    setBusy(true);
    setError(null);
    setDone(null);
    try {
      const { default: readXlsxFile } = await import('read-excel-file/browser');
      const sheets = await readXlsxFile(file);
      const data = (sheets[0]?.data ?? []) as Cell[][];
      const read = readSheet(data);
      const headerRow = data.findIndex((r) => r.some((c) => typeof c === 'string' && /name/i.test(c)));
      const raw = new Map<number, Cell[]>();
      data.forEach((r, i) => raw.set(i + 1, r));
      const report = read.rows.length ? await api.importProducts(read.rows, false) : null;
      setLoaded({
        file: file.name,
        rows: read.rows,
        raw,
        header: data[Math.max(0, headerRow)] ?? [],
        clientProblems: read.problems,
        missing: read.missing,
        report,
      });
    } catch (e) {
      setError(describeError(e, lang));
      setLoaded(null);
    } finally {
      setBusy(false);
      if (fileInput.current) fileInput.current.value = '';
    }
  };

  const problems = loaded
    ? [
        ...loaded.clientProblems,
        ...(loaded.report?.rows.filter((r) => r.status === 'error').map((r) => ({ row: r.row, name: r.name, message: r.message ?? '' })) ?? []),
      ].sort((a, b) => a.row - b.row)
    : [];
  const good = loaded?.report ? loaded.report.newCount + loaded.report.updateCount : 0;

  const downloadFixes = async () => {
    if (!loaded) return;
    const { default: writeXlsxFile } = await import('write-excel-file/browser');
    const header = [...loaded.header.map((c) => ({ value: c === null || c === undefined ? '' : String(c), fontWeight: 'bold' as const })), { value: 'Problem', fontWeight: 'bold' as const }];
    const rows = problems.map((p) => [
      ...(loaded.raw.get(p.row) ?? []).map((c) => (c instanceof Date ? c : c === null || c === undefined ? null : typeof c === 'boolean' ? String(c) : c)),
      `Row ${p.row}: ${p.message}`,
    ]);
    const path = await saveFile('products-to-fix.xlsx', await writeXlsxFile([header, ...rows]).toBlob());
    if (path) setSaved(t('savedTo', { file: path }));
  };

  const commit = async () => {
    if (!loaded) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api.importProducts(loaded.rows, true);
      setDone(t('importDone', { new: r.newCount, updated: r.updateCount, skipped: r.errorCount + loaded.clientProblems.length }));
      setLoaded({ ...loaded, report: r });
    } catch (e) {
      setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="bo-head">
        <div>
          <span className="small">
            <button type="button" className="btn" style={{ height: 'auto', padding: 0, border: 0, background: 'none', color: 'var(--brand)' }} onClick={() => go({ name: 'inventory' })}>
              {t('navInventory')}
            </button>{' '}
            › {t('importExcel')}
          </span>
          <h1>{t('importTitle')}</h1>
          <span className="sub">{t('importHelp')}</span>
        </div>
      </div>

      <div className="cols">
        <section className="card grow">
          <ol style={{ margin: 0, paddingInlineStart: 22, display: 'flex', flexDirection: 'column', gap: 6 }}>
            <li>{t('importStep1')}</li>
            <li>{t('importStep2')}</li>
            <li>{t('importStep3')}</li>
          </ol>
          <div>
            <button type="button" className="btn" onClick={() => void downloadTemplate().catch((e) => setError(String(e)))}>
              {t('downloadTemplate')}
            </button>
          </div>
          <div className="dropzone">
            <input
              ref={fileInput}
              id="import-file"
              type="file"
              accept=".xlsx,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
              className="visually-hidden"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void choose(f);
              }}
            />
            <label htmlFor="import-file" className="btn btn-primary" style={{ cursor: 'pointer' }}>
              {busy ? t('readingFile') : t('chooseFile')}
            </label>
            <span className="small muted">{t('oneCatalogue')}</span>
          </div>
          {error && <div className="alert alert-bad">{error}</div>}
          {saved && <div className="alert alert-ok small" role="status">{saved}</div>}
          {done && (
            <div className="alert alert-ok" role="status">
              {done}{' '}
              <button type="button" className="btn" style={{ height: 34, marginInlineStart: 8 }} onClick={() => go({ name: 'inventory' })}>
                {t('navInventory')}
              </button>
            </div>
          )}
        </section>

        {loaded && (
          <section className="card aside" aria-live="polite">
            <strong className="ltr" style={{ textAlign: 'start' }}>
              {t('rowsInFile', { file: loaded.file, n: loaded.rows.length + loaded.clientProblems.length })}
            </strong>
            {loaded.missing.length > 0 && <div className="alert alert-bad small">{t('missingCols', { cols: loaded.missing.join(', ') })}</div>}
            {loaded.report && (
              <>
                <div className="stat">
                  <span>{t('readyNew')}</span>
                  <strong className="mono">{loaded.report.newCount}</strong>
                </div>
                <div className="stat">
                  <span>{t('willUpdate')}</span>
                  <strong className="mono">{loaded.report.updateCount}</strong>
                </div>
              </>
            )}
            <div className="stat">
              <span>{t('needFixing')}</span>
              <strong className="mono" style={{ color: problems.length ? 'var(--bad)' : undefined }}>
                {problems.length}
              </strong>
            </div>
            {problems.length > 0 && (
              <div className="result-list">
                {problems.slice(0, 200).map((p) => (
                  <div key={`${p.row}-${p.message}`} className="list-row small" style={{ padding: '8px 10px', alignItems: 'flex-start' }}>
                    <span className="tag tag-bad">{t('rowN', { n: p.row })}</span>
                    <span className="ltr" style={{ flex: 1, textAlign: 'start' }}>
                      {p.name ? `${p.name}: ` : ''}
                      {p.message}
                    </span>
                  </div>
                ))}
              </div>
            )}
            {problems.length > 0 && (
              <button type="button" className="btn" onClick={() => void downloadFixes().catch((e) => setError(String(e)))}>
                {t('downloadFix')}
              </button>
            )}
            {!loaded.report?.committed && good > 0 && (
              <button type="button" className="btn btn-primary btn-lg" disabled={busy} onClick={() => void commit()}>
                {t('importNow', { n: good })}
              </button>
            )}
          </section>
        )}
      </div>
    </>
  );
}
