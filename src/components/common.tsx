import type { Language } from '../api/types';

export function BrandMark({ size = 24 }: { size?: number }) {
  return (
    <div className="brand-mark" aria-hidden="true">
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.3" strokeLinecap="round">
        <path d="M12 5v14M5 12h14" />
      </svg>
    </div>
  );
}

export function LanguageToggle({
  value,
  onChange,
  onNight = false,
}: {
  value: Language;
  onChange: (lang: Language) => void;
  onNight?: boolean;
}) {
  return (
    <div role="group" aria-label="Language / زبان" className={`lang${onNight ? ' on-night' : ''}`}>
      <button type="button" className="en" lang="en" aria-pressed={value === 'en'} onClick={() => onChange('en')}>
        English
      </button>
      <button type="button" className="ur" lang="ur" aria-pressed={value === 'ur'} onClick={() => onChange('ur')}>
        اردو
      </button>
    </div>
  );
}

export function TrashIcon() {
  return (
    <svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#B42318" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" />
    </svg>
  );
}

export function SearchIcon() {
  return (
    <svg aria-hidden="true" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="#4A564F" strokeWidth="2" strokeLinecap="round" style={{ flex: 'none' }}>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </svg>
  );
}
