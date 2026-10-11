import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { FbrStatus } from '../api/types';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import { formatDateTime } from '../print/doc';

/** FBR (PRAL) invoice reporting settings (Settings page). Off unless the owner switches it on. */
export function FbrCard() {
  const { t, lang } = useI18n();
  const [status, setStatus] = useState<FbrStatus | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [mode, setMode] = useState<'sandbox' | 'live'>('sandbox');
  const [posId, setPosId] = useState('');
  const [token, setToken] = useState('');
  const [pct, setPct] = useState('');
  const [url, setUrl] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const load = (s: FbrStatus) => {
    setStatus(s);
    setEnabled(s.enabled);
    setMode(s.mode);
    setPosId(s.posId ?? '');
    setPct(s.defaultPct);
    setUrl(s.url ?? '');
    setToken('');
  };

  useEffect(() => {
    api.fbr().then(load).catch((e) => setError(describeError(e, lang)));
  }, [lang]);

  const run = async (f: () => Promise<FbrStatus>, done?: () => void) => {
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      load(await f());
      done?.();
    } catch (e) {
      setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  if (!status) return <section className="card">{error ?? t('loading')}</section>;

  const save = () =>
    run(
      () => api.saveFbr({ enabled, mode, posId, token: token || undefined, defaultPct: pct, url: url || undefined }),
      () => setSaved(true),
    );

  return (
    <section className="card" aria-label={t('fbrTitle')}>
      <div className="card-head">
        <h2>{t('fbrTitle')}</h2>
        <span className={`tag ${status.enabled ? (status.lastError ? 'tag-warn' : 'tag-ok') : 'tag-mute'}`}>
          {status.enabled ? (status.mode === 'live' ? t('fbrLive') : t('fbrSandbox')) : t('offLabel')}
        </span>
      </div>
      <span className="small">{t('fbrHelp')}</span>
      <div className="alert alert-warn small">{t('fbrUntested')}</div>

      {status.enabled && (
        <>
          <div className="stat">
            <span>{t('fbrWaiting')}</span>
            <span className="mono">{status.waiting}</span>
          </div>
          <div className="stat">
            <span>{t('fbrLastSent')}</span>
            <span className="ltr">{status.lastSentAt ? formatDateTime(status.lastSentAt) : t('fbrNever')}</span>
          </div>
          {status.lastError && <div className="alert alert-bad small">{t('fbrError', { msg: status.lastError })}</div>}
          {status.waiting > 0 && (
            <button type="button" className="btn" disabled={busy} onClick={() => void run(api.sendFbr)}>
              {t('fbrSendNow')}
            </button>
          )}
        </>
      )}

      <label className="row" style={{ justifyContent: 'flex-start', gap: 10 }}>
        <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
        <strong>{t('fbrSwitchOn')}</strong>
      </label>
      <div className="field">
        <span className="label">{t('fbrMode')}</span>
        <div className="seg small" role="group" aria-label={t('fbrMode')}>
          <button type="button" aria-pressed={mode === 'sandbox'} onClick={() => setMode('sandbox')}>{t('fbrSandbox')}</button>
          <button type="button" aria-pressed={mode === 'live'} onClick={() => setMode('live')}>{t('fbrLive')}</button>
        </div>
      </div>
      <div className="field">
        <label className="label" htmlFor="fbr-pos">{t('fbrPosId')}</label>
        <input id="fbr-pos" className="input ltr" inputMode="numeric" value={posId} onChange={(e) => setPosId(e.target.value.replace(/\D/g, ''))} />
      </div>
      <div className="field">
        <label className="label" htmlFor="fbr-token">{t('fbrToken')}</label>
        <input
          id="fbr-token"
          className="input ltr"
          type="password"
          autoComplete="off"
          placeholder={status.hasToken ? '••••••••' : ''}
          value={token}
          onChange={(e) => setToken(e.target.value)}
        />
        {status.hasToken && <span className="tiny muted">{t('fbrTokenKeep')}</span>}
      </div>
      <div className="field">
        <label className="label" htmlFor="fbr-pct">{t('fbrPct')}</label>
        <input id="fbr-pct" className="input ltr" value={pct} onChange={(e) => setPct(e.target.value)} />
        <span className="tiny muted">{t('fbrPctHelp')}</span>
      </div>
      <details>
        <summary className="small">{t('fbrUrl')}</summary>
        <input
          aria-label={t('fbrUrl')}
          className="input ltr"
          style={{ marginTop: 8, width: '100%' }}
          placeholder="https://gw.fbr.gov.pk/imsp/v1/api/Live/PostData"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
        />
      </details>
      <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void save()}>
        {busy ? t('loading') : t('fbrSave')}
      </button>
      {saved && <div className="alert alert-ok small" role="status">{t('fbrSaved')}</div>}
      {error && <div className="alert alert-bad" role="alert">{error}</div>}
    </section>
  );
}
