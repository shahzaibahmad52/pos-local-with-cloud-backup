import { useEffect, useState, type FormEvent } from 'react';
import { api } from '../api/client';
import type { ApprovalInput, LoginTile } from '../api/types';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';

/**
 * Asks a manager (owner or a cashier with an approver PIN) to allow
 * something. `onApprove` retries the action with their PIN; if it fails the
 * error stays in the dialog.
 */
export function ApprovalDialog({
  message,
  onApprove,
  onCancel,
}: {
  message: string;
  onApprove: (approval: ApprovalInput) => Promise<void>;
  onCancel: () => void;
}) {
  const { t, lang } = useI18n();
  const [approvers, setApprovers] = useState<LoginTile[]>([]);
  const [who, setWho] = useState('');
  const [secret, setSecret] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .loginTiles()
      .then((list) => {
        const can = list.filter((u) => u.canApprove);
        setApprovers(can);
        if (can[0]) setWho(can[0].id);
      })
      .catch((e) => setError(describeError(e, lang)));
  }, [lang]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!who || !secret || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onApprove({ userId: who, secret });
    } catch (err) {
      setError(describeError(err, lang));
      setSecret('');
      setBusy(false);
    }
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
        {error && <div className="alert alert-bad" role="alert">{error}</div>}
        <div style={{ display: 'flex', gap: 8 }}>
          <button type="button" className="btn" onClick={onCancel}>{t('cancel')}</button>
          <button type="submit" className="btn btn-primary" style={{ flex: 1 }} disabled={busy || !secret}>{t('approve')}</button>
        </div>
      </form>
    </div>
  );
}
