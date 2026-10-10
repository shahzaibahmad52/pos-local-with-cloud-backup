import { useState, type FormEvent } from 'react';
import { api, ApiError } from '../api/client';
import type { ApprovalInput } from '../api/types';
import { parseRupees } from '../domain/money';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import { ApprovalDialog } from './ApprovalDialog';

/** Cash put into or taken out of the drawer during a shift. */
export function CashMoveDialog({ tillCode, onDone, onCancel }: { tillCode: string; onDone: () => void; onCancel: () => void }) {
  const { t, lang } = useI18n();
  const [kind, setKind] = useState<'in' | 'out'>('out');
  const [amount, setAmount] = useState('');
  const [reason, setReason] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [approval, setApproval] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async (withApproval?: ApprovalInput) => {
    const paisa = parseRupees(amount);
    if (paisa === null || paisa <= 0) {
      setError(t('amountRs'));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.cashMove({ tillCode, kind, amount: paisa, reason, approval: withApproval });
      onDone();
    } catch (e) {
      setBusy(false);
      if (e instanceof ApiError && e.code === 'approval_required' && !withApproval) setApproval(describeError(e, lang));
      else if (withApproval) throw e;
      else setError(describeError(e, lang));
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void save();
  };

  if (approval) {
    return <ApprovalDialog message={approval} onApprove={(a) => save(a)} onCancel={() => setApproval(null)} />;
  }

  return (
    <div className="overlay" role="dialog" aria-modal="true" aria-labelledby="cash-title">
      <form className="dialog" onSubmit={submit}>
        <h2 id="cash-title">{t('cashMoveTitle')}</h2>
        <div className="seg" role="group" aria-label={t('cashMoveTitle')}>
          <button type="button" aria-pressed={kind === 'out'} onClick={() => setKind('out')}>{t('cashOutLabel')}</button>
          <button type="button" aria-pressed={kind === 'in'} onClick={() => setKind('in')}>{t('cashInLabel')}</button>
        </div>
        <div className="field">
          <label className="label" htmlFor="cash-amount">{t('amountRs')}</label>
          <input id="cash-amount" className="input big ltr" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} autoFocus />
        </div>
        <div className="field">
          <label className="label" htmlFor="cash-reason">{t('whatFor')}</label>
          <input id="cash-reason" className="input" value={reason} onChange={(e) => setReason(e.target.value)} placeholder={t('whatForHelp')} />
        </div>
        {error && <div className="alert alert-bad" role="alert">{error}</div>}
        <div style={{ display: 'flex', gap: 8 }}>
          <button type="button" className="btn" onClick={onCancel}>{t('cancel')}</button>
          <button type="submit" className="btn btn-primary" style={{ flex: 1 }} disabled={busy}>{t('save')}</button>
        </div>
      </form>
    </div>
  );
}
