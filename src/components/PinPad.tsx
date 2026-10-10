import { useEffect } from 'react';
import { useI18n } from '../i18n/i18n';

/** On-screen keypad for 4-digit PINs; the keyboard's digits work too. */
export function PinPad({
  value,
  onChange,
  onComplete,
  length = 4,
}: {
  value: string;
  onChange: (pin: string) => void;
  onComplete?: (pin: string) => void;
  length?: number;
}) {
  const { t } = useI18n();

  const press = (key: string) => {
    if (key === 'clear') return onChange('');
    if (key === 'del') return onChange(value.slice(0, -1));
    if (value.length >= length) return;
    const next = value + key;
    onChange(next);
    if (next.length === length) onComplete?.(next);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (/^[0-9]$/.test(e.key)) press(e.key);
      else if (e.key === 'Backspace') press('del');
      else if (e.key === 'Escape') press('clear');
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const keys = ['1', '2', '3', '4', '5', '6', '7', '8', '9', 'clear', '0', 'del'];
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div className="dots" aria-label={`${value.length} of ${length} digits entered`}>
        {Array.from({ length }, (_, i) => (
          <span key={i} className={`dot${i < value.length ? ' on' : ''}`} />
        ))}
      </div>
      <div className="pinpad">
        {keys.map((k) => (
          <button
            key={k}
            type="button"
            onClick={() => press(k)}
            aria-label={k === 'del' ? 'Delete last digit' : k === 'clear' ? 'Clear PIN' : `Digit ${k}`}
            className={k.length === 1 ? 'mono' : undefined}
          >
            {k === 'clear' ? t('clear') : k === 'del' ? t('del') : k}
          </button>
        ))}
      </div>
    </div>
  );
}
