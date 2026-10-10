// Draws a Doc onto a canvas and turns it into the 1-bit image ESC/POS
// printers take. Drawing the text ourselves means Urdu prints on any
// thermal printer, even one without an Urdu font.

import type { Block, Doc } from './doc';

/** Packs RGBA pixels into rows of bits, 1 = black dot, most significant bit first. */
export function packBits(rgba: Uint8ClampedArray | Uint8Array, width: number, height: number, threshold = 160): Uint8Array {
  const rowBytes = Math.ceil(width / 8);
  const out = new Uint8Array(rowBytes * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      const alpha = rgba[i + 3] / 255;
      const lum = (0.299 * rgba[i] + 0.587 * rgba[i + 1] + 0.114 * rgba[i + 2]) * alpha + 255 * (1 - alpha);
      if (lum < threshold) out[y * rowBytes + (x >> 3)] |= 0x80 >> (x & 7);
    }
  }
  return out;
}

export function toBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

/** Direction of the first letter in `text` (numbers and punctuation don't count). */
export function firstStrongDir(text: string): 'ltr' | 'rtl' | null {
  for (const ch of text) {
    if (/[\u0590-\u08FF\uFB1D-\uFDFF\uFE70-\uFEFF]/.test(ch)) return 'rtl';
    if (/[A-Za-z\u00C0-\u024F]/.test(ch)) return 'ltr';
  }
  return null;
}

const MONO = '"IBM Plex Mono"';
const URDU = '"Noto Nastaliq Urdu"';

async function ensureFonts(): Promise<void> {
  if (typeof document === 'undefined' || !document.fonts) return;
  await Promise.all([
    document.fonts.load(`400 22px ${MONO}`),
    document.fonts.load(`700 22px ${MONO}`),
    document.fonts.load(`400 22px ${URDU}`, 'اردو'),
    document.fonts.load(`700 22px ${URDU}`, 'اردو'),
  ]).catch(() => undefined);
}

/** Splits text into lines no wider than `max` pixels. */
function wrap(ctx: CanvasRenderingContext2D, text: string, max: number): string[] {
  const lines: string[] = [];
  for (const paragraph of text.split('\n')) {
    let line = '';
    for (const word of paragraph.split(' ')) {
      const attempt = line ? `${line} ${word}` : word;
      if (ctx.measureText(attempt).width <= max) {
        line = attempt;
        continue;
      }
      if (line) lines.push(line);
      // A single word wider than the paper: break it by characters.
      let piece = '';
      for (const ch of word) {
        if (ctx.measureText(piece + ch).width > max && piece) {
          lines.push(piece);
          piece = ch;
        } else {
          piece += ch;
        }
      }
      line = piece;
    }
    lines.push(line);
  }
  return lines;
}

/** Draws the document `width` dots wide (576 for 80 mm paper, 384 for 58 mm). */
export async function renderDoc(doc: Doc, width: number): Promise<HTMLCanvasElement> {
  await ensureFonts();
  const scale = width / 576;
  const size = Math.round(22 * scale);
  const small = Math.round(18 * scale);
  const big = Math.round(30 * scale);
  const margin = Math.round(6 * scale);
  const inner = width - margin * 2;
  const rtl = doc.dir === 'rtl';
  const family = (ltr?: boolean) => (ltr || !rtl ? `${MONO}, ${URDU}, monospace` : `${URDU}, ${MONO}, serif`);
  const lineHeight = (px: number) => Math.round(px * (doc.urdu ? 1.95 : 1.4));

  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = 12_000;
  const ctx = canvas.getContext('2d')!;
  ctx.fillStyle = '#fff';
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = '#000';
  ctx.strokeStyle = '#000';
  ctx.textBaseline = 'alphabetic';

  let y = margin;
  // English-only lines inside an Urdu receipt keep left-to-right order.
  const isLtr = (text: string, forced?: boolean) => forced || !rtl || firstStrongDir(text) === 'ltr';
  const font = (px: number, bold: boolean, ltr?: boolean) => {
    ctx.font = `${bold ? 700 : 400} ${px}px ${family(ltr)}`;
    ctx.direction = ltr || !rtl ? 'ltr' : 'rtl';
  };
  const baseline = (px: number) => y + Math.round(lineHeight(px) * 0.72);

  const draw = (b: Block) => {
    switch (b.kind) {
      case 'title':
      case 'center': {
        const px = b.kind === 'title' ? big : b.small ? small : size;
        font(px, b.kind === 'title' || !!b.bold, isLtr(b.text, b.kind === 'center' && b.ltr));
        ctx.textAlign = 'center';
        for (const line of wrap(ctx, b.text, inner)) {
          ctx.fillText(line, width / 2, baseline(px));
          y += lineHeight(px);
        }
        return;
      }
      case 'rule': {
        const mid = y + Math.round(size * 0.5);
        ctx.lineWidth = b.heavy ? Math.max(2, Math.round(3 * scale)) : 1;
        ctx.setLineDash(b.heavy ? [] : [Math.round(6 * scale), Math.round(4 * scale)]);
        ctx.beginPath();
        ctx.moveTo(margin, mid);
        ctx.lineTo(width - margin, mid);
        ctx.stroke();
        ctx.setLineDash([]);
        y += size;
        return;
      }
      case 'row': {
        const px = b.big ? big : size;
        font(px, !!b.bold, true);
        const valueWidth = ctx.measureText(b.value).width;
        ctx.textAlign = rtl ? 'left' : 'right';
        ctx.fillText(b.value, rtl ? margin : width - margin, baseline(px));
        font(px, !!b.bold, isLtr(b.label));
        const lines = wrap(ctx, b.label, Math.max(40, inner - valueWidth - 12 * scale));
        ctx.textAlign = rtl ? 'right' : 'left';
        for (const line of lines) {
          ctx.fillText(line, rtl ? width - margin : margin, baseline(px));
          y += lineHeight(px);
        }
        return;
      }
      case 'text': {
        font(size, !!b.bold, isLtr(b.text, b.ltr));
        const start = b.ltr || !rtl;
        ctx.textAlign = start ? 'left' : 'right';
        for (const line of wrap(ctx, b.text, inner)) {
          ctx.fillText(line, start ? margin : width - margin, baseline(size));
          y += lineHeight(size);
        }
        return;
      }
    }
  };
  for (const block of doc.blocks) draw(block);

  const height = Math.min(canvas.height, y + Math.round(24 * scale));
  const out = document.createElement('canvas');
  out.width = width;
  out.height = height;
  out.getContext('2d')!.drawImage(canvas, 0, 0);
  return out;
}
