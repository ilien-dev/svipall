/**
 * A GIF89a encoder, and the PNG reader that feeds it. No dependency.
 *
 * There is a reason this is here rather than an ffmpeg invocation. The ffmpeg
 * Playwright unpacks is built `--disable-everything`: it has no GIF muxer, no
 * palettegen and no image2 demuxer, so it cannot do this job at all. Asking
 * for a second ffmpeg would make a repository that installs nothing depend on
 * an installed binary to rebuild a picture of itself.
 *
 * What it does that a generic encoder would not:
 *
 * - One global palette, built from a histogram of the frames themselves. The
 *   demo has a dozen declared colours and the rest is greyscale antialiasing,
 *   so an exact palette usually fits and nothing is approximated at all.
 * - No dithering. Dither on 15px text is noise that costs bytes and legibility.
 * - Every frame after the first is written as the rectangle that changed, over
 *   a frame left in place. A terminal changes one line at a time, and this is
 *   most of the reason the file is small.
 */

import { inflateSync } from 'node:zlib';

/* ---- PNG ----------------------------------------------------------------- */

/** Chrome screenshots: 8-bit, non-interlaced, colour type 2 or 6. Anything
 *  else is a bug in the caller rather than a case to handle. */
export const decodePng = (buf) => {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error('not a PNG');

  let pos = 8;
  let width = 0;
  let height = 0;
  let channels = 0;
  const idat = [];

  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.toString('ascii', pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    pos += 12 + len;

    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      const depth = data[8];
      const colour = data[9];
      const interlace = data[12];
      if (depth !== 8 || interlace !== 0 || (colour !== 2 && colour !== 6)) {
        throw new Error(`unsupported PNG: depth ${depth} colour ${colour} interlace ${interlace}`);
      }
      channels = colour === 6 ? 4 : 3;
    } else if (type === 'IDAT') {
      idat.push(data);
    } else if (type === 'IEND') {
      break;
    }
  }

  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const out = Buffer.allocUnsafe(height * stride);

  /* Un-filter in place, one scanline at a time (RFC 2083 section 6). */
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const src = y * (stride + 1) + 1;
    const dst = y * stride;
    const up = dst - stride;

    for (let x = 0; x < stride; x += 1) {
      const byte = raw[src + x];
      const a = x >= channels ? out[dst + x - channels] : 0;
      const b = y > 0 ? out[up + x] : 0;
      const c = x >= channels && y > 0 ? out[up + x - channels] : 0;
      let value;
      switch (filter) {
        case 0: value = byte; break;
        case 1: value = byte + a; break;
        case 2: value = byte + b; break;
        case 3: value = byte + ((a + b) >> 1); break;
        case 4: {
          const p = a + b - c;
          const pa = Math.abs(p - a);
          const pb = Math.abs(p - b);
          const pc = Math.abs(p - c);
          value = byte + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c);
          break;
        }
        default: throw new Error(`unknown PNG filter ${filter}`);
      }
      out[dst + x] = value & 0xff;
    }
  }

  return { width, height, channels, data: out };
};

/** Exact 2x box downscale. The recording is taken at deviceScaleFactor 2 for
 *  the sake of the text, and averaging four pixels is both the right filter
 *  for that and the one that invents no new colours off the edges. */
export const halve = ({ width, height, channels, data }) => {
  const w = width >> 1;
  const h = height >> 1;
  const out = Buffer.allocUnsafe(w * h * 3);

  for (let y = 0; y < h; y += 1) {
    for (let x = 0; x < w; x += 1) {
      const a = (y * 2 * width + x * 2) * channels;
      const b = a + channels;
      const c = a + width * channels;
      const d = c + channels;
      const o = (y * w + x) * 3;
      for (let k = 0; k < 3; k += 1) {
        out[o + k] = (data[a + k] + data[b + k] + data[c + k] + data[d + k] + 2) >> 2;
      }
    }
  }

  return { width: w, height: h, data: out };
};

/* ---- palette ------------------------------------------------------------- */

const key = (r, g, b) => (r << 16) | (g << 8) | b;

/** Median cut over a histogram, which is the whole image only when it has to
 *  be: if the frames hold 256 colours or fewer the palette is exact. */
export const buildPalette = (histogram, max = 256) => {
  const colours = [...histogram.keys()];
  if (colours.length <= max) {
    return colours.map((k) => [(k >> 16) & 0xff, (k >> 8) & 0xff, k & 0xff]);
  }

  let boxes = [colours];
  while (boxes.length < max) {
    /* Split the box with the widest channel, weighted by how many pixels are
       actually in it: a wide box nobody looks at is not worth a slot. */
    let pick = -1;
    let best = -1;
    for (let i = 0; i < boxes.length; i += 1) {
      if (boxes[i].length < 2) continue;
      let lo = [255, 255, 255];
      let hi = [0, 0, 0];
      let n = 0;
      for (const k of boxes[i]) {
        n += histogram.get(k);
        for (let c = 0; c < 3; c += 1) {
          const v = (k >> (16 - c * 8)) & 0xff;
          if (v < lo[c]) lo[c] = v;
          if (v > hi[c]) hi[c] = v;
        }
      }
      const spread = Math.max(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
      const score = spread * Math.log2(n + 1);
      if (score > best) {
        best = score;
        pick = i;
      }
    }
    if (pick === -1) break;

    const box = boxes[pick];
    let lo = [255, 255, 255];
    let hi = [0, 0, 0];
    for (const k of box) {
      for (let c = 0; c < 3; c += 1) {
        const v = (k >> (16 - c * 8)) & 0xff;
        if (v < lo[c]) lo[c] = v;
        if (v > hi[c]) hi[c] = v;
      }
    }
    const axis = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]].reduce(
      (bestAxis, span, i, all) => (span > all[bestAxis] ? i : bestAxis),
      0,
    );
    const shift = 16 - axis * 8;
    box.sort((p, q) => ((p >> shift) & 0xff) - ((q >> shift) & 0xff));
    const half = box.length >> 1;
    boxes.splice(pick, 1, box.slice(0, half), box.slice(half));
  }

  return boxes.map((box) => {
    let r = 0;
    let g = 0;
    let b = 0;
    let n = 0;
    for (const k of box) {
      const w = histogram.get(k);
      r += ((k >> 16) & 0xff) * w;
      g += ((k >> 8) & 0xff) * w;
      b += (k & 0xff) * w;
      n += w;
    }
    return [Math.round(r / n), Math.round(g / n), Math.round(b / n)];
  });
};

/** Nearest palette entry, memoised. Squared distance in RGB: the palette is
 *  built from these same pixels, so the nearest entry is almost always exact
 *  and a perceptual metric would cost time to change nothing. */
export const mapper = (palette) => {
  const cache = new Map();
  return (r, g, b) => {
    const k = key(r, g, b);
    const hit = cache.get(k);
    if (hit !== undefined) return hit;
    let best = 0;
    let bestD = Infinity;
    for (let i = 0; i < palette.length; i += 1) {
      const dr = r - palette[i][0];
      const dg = g - palette[i][1];
      const db = b - palette[i][2];
      const d = dr * dr + dg * dg + db * db;
      if (d < bestD) {
        bestD = d;
        best = i;
        if (d === 0) break;
      }
    }
    cache.set(k, best);
    return best;
  };
};

/* ---- LZW ----------------------------------------------------------------- */

/** GIF's variant of LZW: codes are packed low bit first and the width grows
 *  as the dictionary fills, with a clear code whenever it is full. */
const lzw = (indices, minCodeSize) => {
  const clear = 1 << minCodeSize;
  const end = clear + 1;

  const out = [];
  let bits = 0;
  let held = 0;

  const emit = (code, width) => {
    held |= code << bits;
    bits += width;
    while (bits >= 8) {
      out.push(held & 0xff);
      held >>= 8;
      bits -= 8;
    }
  };

  let width = minCodeSize + 1;
  let next = end + 1;
  let dict = new Map();

  emit(clear, width);

  let prefix = indices[0];
  for (let i = 1; i < indices.length; i += 1) {
    const k = indices[i];
    const pair = prefix * 4096 + k;
    const found = dict.get(pair);
    if (found !== undefined) {
      prefix = found;
      continue;
    }
    emit(prefix, width);
    dict.set(pair, next);
    next += 1;
    if (next - 1 === 1 << width && width < 12) width += 1;
    if (next === 4096) {
      emit(clear, width);
      dict = new Map();
      next = end + 1;
      width = minCodeSize + 1;
    }
    prefix = k;
  }

  emit(prefix, width);
  emit(end, width);
  if (bits > 0) out.push(held & 0xff);

  /* Sub-blocks: a length byte then up to 255 bytes, terminated by a zero. */
  const blocks = [];
  for (let i = 0; i < out.length; i += 255) {
    const chunk = out.slice(i, i + 255);
    blocks.push(chunk.length, ...chunk);
  }
  blocks.push(0);
  return Buffer.from(blocks);
};

/* ---- the file ------------------------------------------------------------ */

export class Gif {
  /**
   * @param {number} width
   * @param {number} height
   * @param {[number,number,number][]} palette
   * @param {number} delayCs frame delay in hundredths of a second
   */
  constructor(width, height, palette, delayCs) {
    this.width = width;
    this.height = height;
    this.palette = palette;
    this.delayCs = delayCs;
    this.parts = [];
    this.previous = null;

    const bits = Math.max(1, Math.ceil(Math.log2(Math.max(2, palette.length))));
    this.slots = 1 << bits;

    const head = Buffer.alloc(13);
    head.write('GIF89a', 0, 'ascii');
    head.writeUInt16LE(width, 6);
    head.writeUInt16LE(height, 8);
    head[10] = 0x80 | (bits - 1); /* global palette, `bits` per pixel */
    head[11] = 0;
    head[12] = 0;
    this.parts.push(head);

    const table = Buffer.alloc(this.slots * 3);
    palette.forEach(([r, g, b], i) => {
      table[i * 3] = r;
      table[i * 3 + 1] = g;
      table[i * 3 + 2] = b;
    });
    this.parts.push(table);

    /* NETSCAPE2.0: loop forever. */
    this.parts.push(Buffer.from([
      0x21, 0xff, 0x0b,
      ...Buffer.from('NETSCAPE2.0', 'ascii'),
      0x03, 0x01, 0x00, 0x00, 0x00,
    ]));
  }

  /** @param {Uint8Array} indices one palette index per pixel, row major */
  addFrame(indices) {
    let x0 = 0;
    let y0 = 0;
    let w = this.width;
    let h = this.height;

    if (this.previous) {
      const box = this.#changed(indices);
      if (!box) {
        /* Nothing moved. Give the last frame more time instead of writing a
           frame that says nothing. */
        this.#extendLastDelay();
        return;
      }
      [x0, y0, w, h] = box;
    }

    const sub = new Uint8Array(w * h);
    for (let y = 0; y < h; y += 1) {
      sub.set(indices.subarray((y0 + y) * this.width + x0, (y0 + y) * this.width + x0 + w), y * w);
    }

    const gce = Buffer.alloc(8);
    gce[0] = 0x21;
    gce[1] = 0xf9;
    gce[2] = 0x04;
    gce[3] = 0x04; /* disposal 1: leave the frame in place */
    gce.writeUInt16LE(this.delayCs, 4);
    gce[6] = 0;
    gce[7] = 0;
    this.parts.push(gce);
    this.lastGce = gce;

    const desc = Buffer.alloc(10);
    desc[0] = 0x2c;
    desc.writeUInt16LE(x0, 1);
    desc.writeUInt16LE(y0, 3);
    desc.writeUInt16LE(w, 5);
    desc.writeUInt16LE(h, 7);
    desc[9] = 0;
    this.parts.push(desc);

    const minCodeSize = Math.max(2, Math.ceil(Math.log2(this.slots)));
    this.parts.push(Buffer.from([minCodeSize]));
    this.parts.push(lzw(sub, minCodeSize));

    this.previous = Uint8Array.from(indices);
  }

  /** The tightest rectangle covering every pixel that differs from the last
   *  frame, or null when none does. */
  #changed(indices) {
    const { width, height, previous } = this;
    let top = -1;
    let bottom = -1;
    let left = width;
    let right = -1;

    for (let y = 0; y < height; y += 1) {
      const row = y * width;
      let rowLeft = -1;
      let rowRight = -1;
      for (let x = 0; x < width; x += 1) {
        if (indices[row + x] !== previous[row + x]) {
          if (rowLeft === -1) rowLeft = x;
          rowRight = x;
        }
      }
      if (rowLeft !== -1) {
        if (top === -1) top = y;
        bottom = y;
        if (rowLeft < left) left = rowLeft;
        if (rowRight > right) right = rowRight;
      }
    }

    if (top === -1) return null;
    return [left, top, right - left + 1, bottom - top + 1];
  }

  #extendLastDelay() {
    if (!this.lastGce) return;
    this.lastGce.writeUInt16LE(this.lastGce.readUInt16LE(4) + this.delayCs, 4);
  }

  finish() {
    return Buffer.concat([...this.parts, Buffer.from([0x3b])]);
  }
}
