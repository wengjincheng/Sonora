// Generates a 1024x1024 app icon PNG without any native deps (pure node + zlib).
// Design: indigo->violet rounded square with a white music note.
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const W = 1024;
const H = 1024;

const buf = Buffer.alloc(W * H * 4);

function setPx(x, y, r, g, b, a) {
  if (x < 0 || y < 0 || x >= W || y >= H) return;
  const i = (y * W + x) * 4;
  buf[i] = r;
  buf[i + 1] = g;
  buf[i + 2] = b;
  buf[i + 3] = a;
}

const R = 232; // corner radius
function insideRounded(x, y) {
  if (x < 0 || y < 0 || x >= W || y >= H) return false;
  const cx = Math.min(Math.max(x, R), W - R);
  const cy = Math.min(Math.max(y, R), H - R);
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= R * R + 1; // +1 for AA edge
}

// gradient #4F46E5 -> #7C3AED with subtle diagonal
function bgColor(x, y) {
  const t = (x / W) * 0.35 + (y / H) * 0.65;
  const r = Math.round(79 + (124 - 79) * t);
  const g = Math.round(70 + (58 - 70) * t);
  const b = Math.round(229 + (237 - 229) * t);
  return [r, g, b];
}

// music note geometry (anti-aliased via supersampling factor 3)
const SS = 3;
function noteShape(x, y) {
  // note head: ellipse centered (438, 700), rx 118, ry 92, rotated -18deg
  const hx = x - 438;
  const hy = y - 700;
  const c = Math.cos((-18 * Math.PI) / 180);
  const s = Math.sin((-18 * Math.PI) / 180);
  const rx = hx * c - hy * s;
  const ry = hx * s + hy * c;
  const head = (rx / 120) ** 2 + (ry / 94) ** 2 <= 1;

  // stem: from head right edge up
  const stem = x >= 538 && x <= 578 && y >= 240 && y <= 712;

  // flag: thick bar sloping down from stem top
  const flag = x > 578 && x <= 810 && y >= 240 && y <= 240 + 56 + (x - 578) * 0.42;

  // second note head (chord): ellipse centered (688, 742)
  const jx = x - 688;
  const jy = y - 742;
  const jrx = jx * c - jy * s;
  const jry = jx * s + jy * c;
  const head2 = (jrx / 120) ** 2 + (jry / 94) ** 2 <= 1;
  const stem2 = x >= 788 && x <= 828 && y >= 240 && y <= 754;

  return head || stem || flag || head2 || stem2;
}

for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    if (!insideRounded(x, y)) {
      setPx(x, y, 0, 0, 0, 0);
      continue;
    }
    const [br, bg, bb] = bgColor(x, y);
    // supersample note coverage
    let cov = 0;
    for (let sy = 0; sy < SS; sy++) {
      for (let sx = 0; sx < SS; sx++) {
        if (noteShape(x + (sx + 0.5) / SS, y + (sy + 0.5) / SS)) cov++;
      }
    }
    const a = cov / (SS * SS);
    const r = Math.round(br + (255 - br) * a);
    const g = Math.round(bg + (255 - bg) * a);
    const b = Math.round(bb + (255 - bb) * a);
    setPx(x, y, r, g, b, 255);
  }
}

// ---- minimal PNG encoder ----
function crc32(b) {
  let table = crc32.table;
  if (!table) {
    table = crc32.table = new Int32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      table[n] = c;
    }
  }
  let c = -1;
  for (let i = 0; i < b.length; i++) c = table[(c ^ b[i]) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0);
ihdr.writeUInt32BE(H, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
// filter bytes per scanline
const raw = Buffer.alloc((W * 4 + 1) * H);
for (let y = 0; y < H; y++) {
  raw[y * (W * 4 + 1)] = 0;
  buf.copy(raw, y * (W * 4 + 1) + 1, y * W * 4, (y + 1) * W * 4);
}
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

const out = resolve(dirname(fileURLToPath(import.meta.url)), "../assets/icon-src.png");
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, png);
console.log("wrote", out, png.length, "bytes");
