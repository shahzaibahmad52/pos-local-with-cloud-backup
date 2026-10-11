import { createPortal } from 'react-dom';
import type { Doc } from './doc';
import { qrModules } from './qr';

function QrCode({ text }: { text: string }) {
  const m = qrModules(text);
  const n = m.length + 8;
  const path = m.flatMap((row, y) => row.map((dark, x) => (dark ? `M${x + 4} ${y + 4}h1v1h-1z` : ''))).join('');
  return (
    <svg className="qr" viewBox={`0 0 ${n} ${n}`} width={110} height={110} role="img" aria-label={text} shapeRendering="crispEdges">
      <rect width={n} height={n} fill="#fff" />
      <path d={path} fill="#000" />
    </svg>
  );
}

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
          case 'qr':
            return (
              <div key={i} className="center">
                <QrCode text={b.text} />
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
