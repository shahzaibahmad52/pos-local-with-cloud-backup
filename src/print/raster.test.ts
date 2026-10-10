import { describe, expect, it } from 'vitest';
import { packBits, toBase64 } from './raster';

function image(width: number, height: number, black: (x: number, y: number) => boolean): Uint8Array {
  const px = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const v = black(x, y) ? 0 : 255;
      px.set([v, v, v, 255], (y * width + x) * 4);
    }
  }
  return px;
}

describe('packBits', () => {
  it('packs black pixels most significant bit first', () => {
    const bits = packBits(image(16, 2, (x, y) => (y === 0 ? x < 8 : x % 2 === 0)), 16, 2);
    expect(Array.from(bits)).toEqual([0xff, 0x00, 0xaa, 0xaa]);
  });

  it('pads rows to whole bytes', () => {
    const bits = packBits(image(10, 1, () => true), 10, 1);
    expect(Array.from(bits)).toEqual([0xff, 0xc0]);
  });

  it('treats transparent pixels as paper', () => {
    const px = new Uint8Array([0, 0, 0, 0]);
    expect(Array.from(packBits(px, 1, 1))).toEqual([0]);
  });

  it('base64-encodes', () => {
    expect(toBase64(new Uint8Array([0xff, 0x00, 0xaa]))).toBe('/wCq');
  });
});

describe('firstStrongDir', () => {
  it('finds the direction of the first letter', async () => {
    const { firstStrongDir } = await import('./raster');
    expect(firstStrongDir('2 Strip × 30')).toBe('ltr');
    expect(firstStrongDir('625 g @ 160 / کلو')).toBe('ltr');
    expect(firstStrongDir('علی · T01')).toBe('rtl');
    expect(firstStrongDir('1,290')).toBeNull();
  });
});
