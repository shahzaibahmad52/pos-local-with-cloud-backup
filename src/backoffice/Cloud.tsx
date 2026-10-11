import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { CloudPage } from '../api/types';
import { formatDateTime } from '../print/doc';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';

function size(bytes: number): string {
  return bytes >= 1_048_576 ? `${(bytes / 1_048_576).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/** Cloud backup settings and the daily backup files (Settings page). */
export function CloudBackup({ onChange }: { onChange?: (page: CloudPage) => void }) {
  const { t, lang } = useI18n();
  const [page, setPage] = useState<CloudPage | null>(null);
  const [url, setUrl] = useState('');
  const [token, setToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .cloud()
      .then((p) => {
        setPage(p);
        setUrl(p.url ?? '');
      })
      .catch((e) => setError(describeError(e, lang)));
  }, [lang]);

  const run = async (f: () => Promise<CloudPage>) => {
    setBusy(true);
    setError(null);
    try {
      const next = await f();
      setPage(next);
      onChange?.(next);
      setToken('');
    } catch (e) {
      setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  if (!page) return <section className="card">{error ?? t('loading')}</section>;

  return (
    <>
      <section className="card" aria-label={t('cloudTitle')}>
        <div className="card-head">
          <h2>{t('cloudTitle')}</h2>
          <span className={`tag ${page.enabled ? (page.lastError ? 'tag-warn' : 'tag-ok') : 'tag-mute'}`}>
            {page.enabled ? t('cloudOn') : t('offLabel')}
          </span>
        </div>
        {page.enabled ? (
          <>
            <span className="small muted ltr" style={{ textAlign: 'start', wordBreak: 'break-all' }}>
              {page.url}
            </span>
            <div className="stat">
              <span>{t('cloudLastSync')}</span>
              <span className="ltr">{page.lastSyncAt ? formatDateTime(page.lastSyncAt) : t('cloudNever')}</span>
            </div>
            <div className="stat">
              <span>{t('cloudPending')}</span>
              <span className="mono">{page.pending.toLocaleString('en-US')}</span>
            </div>
            {page.lastError && <div className="alert alert-warn small">{t('cloudError', { msg: page.lastError })}</div>}
            <span className="small muted">{t('cloudWorks')}</span>
            <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void run(api.syncNow)}>
                {busy ? t('loading') : t('cloudSyncNow')}
              </button>
              <button
                type="button"
                className="btn btn-danger"
                disabled={busy}
                onClick={() => window.confirm(t('cloudConfirmOff')) && void run(api.disableCloud)}
              >
                {t('cloudTurnOff')}
              </button>
            </div>
          </>
        ) : (
          <>
            <span className="small">{t('cloudHowTo')}</span>
            <div className="field">
              <label className="label" htmlFor="cl-url">{t('cloudUrl')}</label>
              <input id="cl-url" className="input ltr" placeholder="libsql://store-b1-yourname.turso.io" value={url} onChange={(e) => setUrl(e.target.value)} />
            </div>
            <div className="field">
              <label className="label" htmlFor="cl-token">{t('cloudToken')}</label>
              <input id="cl-token" className="input ltr" type="password" autoComplete="off" value={token} onChange={(e) => setToken(e.target.value)} />
            </div>
            <button type="button" className="btn btn-primary" disabled={busy || !url.trim() || !token.trim()} onClick={() => void run(() => api.enableCloud(url, token))}>
              {busy ? t('loading') : t('cloudTurnOn')}
            </button>
            <span className="tiny muted">{t('cloudOffNote')}</span>
          </>
        )}
        {error && <div className="alert alert-bad" role="alert">{error}</div>}
      </section>

      <section className="card" aria-label={t('localBackups')}>
        <h2>{t('localBackups')}</h2>
        <span className="small">{t('localBackupsHelp')}</span>
        {page.backupDir && (
          <span className="tiny muted ltr" style={{ textAlign: 'start', wordBreak: 'break-all' }}>
            {t('backupFolder', { dir: page.backupDir })}
          </span>
        )}
        {page.backups.length === 0 && <span className="small muted">{t('noBackups')}</span>}
        {page.backups.slice(0, 5).map((b) => (
          <div key={b.name} className="stat">
            <span className="mono small">{b.name}</span>
            <span className="small">{size(b.size)}</span>
          </div>
        ))}
        {page.backupDir && (
          <button type="button" className="btn" disabled={busy} onClick={() => void run(api.backupNow)}>
            {t('backupNowBtn')}
          </button>
        )}
      </section>
    </>
  );
}
