import { createPortal } from 'react-dom';
import type { Doc } from './doc';

/** On-screen 80 mm preview of a printable document. */
export function DocView({ doc }: { doc: Doc }) {
  return (
    <div className={`receipt${doc.urdu ? ' has-ur' : ''}`} dir={doc.dir} lang={doc.dir === 'rtl' ? 'ur' : 'en'}>
      {doc.blocks.map((b, i) => {
        switch (b.kind) {
          case 'title':
            return (
              <div key={i} dir="auto" className="center" style={{ fontSize: 16, fontWeight: 700 }}>
                {b.text}
              </div>
            );
          case 'center':
            return (
              <div
                key={i}
                dir={b.ltr ? 'ltr' : 'auto'}
                className="center"
                style={{ fontWeight: b.bold ? 700 : undefined, fontSize: b.small ? 10.5 : undefined, whiteSpace: 'pre-line' }}
              >
                {b.text}
              </div>
            );
          case 'rule':
            return <div key={i} className="rule" style={b.heavy ? { borderTop: '2px solid #000' } : undefined} />;
          case 'row':
            return (
              <div key={i} className="rrow" style={{ fontWeight: b.bold ? 700 : undefined, fontSize: b.big ? 16 : undefined }}>
                <span dir="auto">{b.label}</span>
                <span className="ltr">{b.value}</span>
              </div>
            );
          case 'text':
            return (
              <div key={i} dir={b.ltr ? 'ltr' : 'auto'} className={b.ltr ? 'pn' : undefined} style={{ fontWeight: b.bold ? 600 : undefined }}>
                {b.text}
              </div>
            );
        }
      })}
    </div>
  );
}

/**
 * The document the browser's print dialog prints. Rendered outside the app
 * root so `@media print` can hide everything else.
 */
export function PrintPortal({ doc }: { doc: Doc | null }) {
  if (!doc) return null;
  return createPortal(
    <div className="print-root">
      <DocView doc={doc} />
    </div>,
    document.body,
  );
}
