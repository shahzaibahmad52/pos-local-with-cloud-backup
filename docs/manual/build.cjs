// Builds docs/Bizflow-POS-User-Manual.pdf from manual.html + cover.html.
//
//   node docs/manual/build.cjs
//
// Needs Playwright's Chromium, `pdftotext` (poppler-utils) and Python's
// `pypdf`. The body is rendered twice: the first pass finds which page each
// heading lands on, the second fills those page numbers into the contents.

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

let chromium;
try {
  ({ chromium } = require('playwright'));
} catch {
  ({ chromium } = require(process.env.PLAYWRIGHT_PATH || '/opt/node-tools/node_modules/playwright'));
}

const DIR = __dirname;
const OUT = path.join(DIR, '..', 'Bizflow-POS-User-Manual.pdf');
const TMP = fs.mkdtempSync(path.join(require('os').tmpdir(), 'bizflow-manual-'));

const strip = (s) => s.replace(/<[^>]+>/g, '').replace(/&amp;/g, '&').trim();

/** Numbers chapters, sections and figures; returns the html and the outline. */
function prepare(src) {
  const outline = [];
  let ch = 0;
  let sec = 0;
  let fig = 0;
  let html = src.replace(/<h([12]) id="([^"]+)">([\s\S]*?)<\/h\1>/g, (_, level, id, text) => {
    if (level === '1') {
      ch += 1;
      sec = 0;
      outline.push({ level: 1, id, n: String(ch), title: strip(text) });
      return `<h1 id="${id}"><span class="mk">QQMK${id}QQ</span><span class="num">Chapter ${ch}</span>${text}</h1>`;
    }
    sec += 1;
    const n = `${ch}.${sec}`;
    outline.push({ level: 2, id, n, title: strip(text) });
    return `<h2 id="${id}"><span class="mk">QQMK${id}QQ</span><span class="num">${n}</span>${text}</h2>`;
  });
  // Keep a heading (and a short intro line) on the same page as its first figure.
  html = html.replace(
    /(<h([23])[^>]*>(?:(?!<\/h\2>)[\s\S])*<\/h\2>\s*(?:<p>(?:(?!<\/p>)[\s\S])*<\/p>\s*)?<figure[^>]*>[\s\S]*?<\/figure>)/g,
    '<div class="keep">$1</div>',
  );
  html = html.replace(/<figcaption>/g, () => `<figcaption><b>Figure ${++fig}</b> · `);
  return { html, outline };
}

function toc(outline, pages) {
  const pg = (id) => (pages[id] ? String(pages[id]) : '');
  let out = '<section class="toc"><h1>Contents</h1><ol>';
  let open = false;
  for (const e of outline) {
    if (e.level === 1) {
      if (open) out += '</ol></li>';
      out += `<li><a href="#${e.id}"><span class="n">${e.n}</span>${e.title}<span class="dots"></span><span class="pg">${pg(e.id)}</span></a><ol>`;
      open = true;
    } else {
      out += `<li><a href="#${e.id}"><span class="n">${e.n}</span>${e.title}<span class="dots"></span><span class="pg">${pg(e.id)}</span></a></li>`;
    }
  }
  if (open) out += '</ol></li>';
  return out + '</ol></section>';
}

const markSvg = fs.readFileSync(path.join(DIR, 'img', 'bizflow-mark.svg'));
const mark = `data:image/svg+xml;base64,${markSvg.toString('base64')}`;
const headerTemplate = `
  <div style="width:100%;margin:0 16mm;padding-bottom:2mm;border-bottom:0.6px solid #d9e0e8;display:flex;align-items:center;gap:6px;font-family:Inter,Arial,sans-serif;font-size:7.5pt;color:#55657a;-webkit-print-color-adjust:exact;">
    <img src="${mark}" style="width:11px;height:11px">
    <span style="font-weight:700;color:#076599">Bizflow POS</span><span>· User Manual</span>
  </div>`;
const footerTemplate = `
  <div style="width:100%;margin:0 16mm;display:flex;justify-content:space-between;font-family:Inter,Arial,sans-serif;font-size:7.5pt;color:#55657a;">
    <span>bizflow — point of sale for medical &amp; general stores</span>
    <span>Page <span class="pageNumber" style="font-weight:700;color:#076599"></span> of <span class="totalPages"></span></span>
  </div>`;

async function renderBody(page, html, file) {
  const p = path.join(DIR, '.render.html');
  fs.writeFileSync(p, html);
  await page.goto('file://' + p, { waitUntil: 'networkidle' });
  await page.pdf({
    path: file,
    format: 'A4',
    printBackground: true,
    displayHeaderFooter: true,
    headerTemplate,
    footerTemplate,
    margin: { top: '20mm', bottom: '18mm', left: '16mm', right: '16mm' },
    outline: true,
  });
  fs.unlinkSync(p);
}

function findPages(file) {
  const text = execFileSync('pdftotext', ['-layout', file, '-'], { encoding: 'utf8', maxBuffer: 64 << 20 });
  const pages = {};
  text.split('\f').forEach((t, i) => {
    for (const m of t.matchAll(/QQMK([\w-]+)QQ/g)) pages[m[1]] ??= i + 1;
  });
  return pages;
}

(async () => {
  const src = fs.readFileSync(path.join(DIR, 'manual.html'), 'utf8');
  const { html, outline } = prepare(src);
  const browser = await chromium.launch();
  const page = await browser.newPage();

  // Pass 1 with blank page numbers (same layout), pass 2 with real ones.
  const pass1 = path.join(TMP, 'pass1.pdf');
  await renderBody(page, html.replace('<!--TOC-->', toc(outline, {})), pass1);
  const pages = findPages(pass1);
  const missing = outline.filter((e) => !pages[e.id]).map((e) => e.id);
  if (missing.length) throw new Error(`Headings not found in the PDF: ${missing.join(', ')}`);

  const body = path.join(TMP, 'body.pdf');
  await renderBody(page, html.replace('<!--TOC-->', toc(outline, pages)).replace(/<span class="mk">QQMK[\w-]+QQ<\/span>/g, ''), body);

  const cover = path.join(TMP, 'cover.pdf');
  await page.goto('file://' + path.join(DIR, 'cover.html'), { waitUntil: 'networkidle' });
  await page.pdf({ path: cover, format: 'A4', printBackground: true, margin: { top: 0, bottom: 0, left: 0, right: 0 } });
  await browser.close();

  execFileSync('python3', ['-c', `
import sys
from pypdf import PdfWriter, PdfReader
w = PdfWriter()
w.append(PdfReader(sys.argv[1]))
w.append(PdfReader(sys.argv[2]))
w.add_metadata({'/Title': 'Bizflow POS — User Manual', '/Author': 'Bizflow', '/Subject': 'User manual for Bizflow POS'})
w.page_mode = '/UseOutlines'
w.compress_identical_objects()
with open(sys.argv[3], 'wb') as f:
    w.write(f)
`, cover, body, OUT]);
  fs.rmSync(TMP, { recursive: true, force: true });
  console.log(`Wrote ${OUT} (${(fs.statSync(OUT).size / 1e6).toFixed(1)} MB)`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
