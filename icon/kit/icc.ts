// A Display P3 ICC profile (v4), built from first principles, and the PNG surgery to embed it. With it, an icon
// whose pixels are encoded in Display P3 shows its full colour on wide-gamut screens, and is converted correctly
// everywhere else.
import { deflateSync } from "node:zlib";

const P3_TO_XYZ_D65 = [
  [0.4865709, 0.2656677, 0.1982173],
  [0.2289746, 0.6917385, 0.0792869],
  [0.0, 0.0451134, 1.0439444],
];
/** Bradford chromatic adaptation from D65 to the ICC's D50. */
const BRADFORD_D65_TO_D50 = [
  [1.0478112, 0.0228866, -0.050127],
  [0.0295424, 0.9904844, -0.0170491],
  [-0.0092345, 0.0150436, 0.7521316],
];
const D50 = [0.9642, 1.0, 0.8249];

const multiply = (a: number[][], b: number[][]) => a.map((row) => b[0].map((_, j) => row.reduce((s, v, k) => s + v * b[k][j], 0)));

const s15 = (v: number) => {
  const b = Buffer.alloc(4);
  b.writeInt32BE(Math.round(v * 65536));
  return b;
};
const sig = (s: string) => Buffer.from(s, "latin1");
const u32 = (v: number) => {
  const b = Buffer.alloc(4);
  b.writeUInt32BE(v);
  return b;
};
const u16 = (v: number) => {
  const b = Buffer.alloc(2);
  b.writeUInt16BE(v);
  return b;
};
const pad4 = (b: Buffer) => (b.length % 4 ? Buffer.concat([b, Buffer.alloc(4 - (b.length % 4))]) : b);

function text(s: string) {
  const utf16 = Buffer.from(s, "utf16le").swap16();
  return Buffer.concat([sig("mluc"), u32(0), u32(1), u32(12), sig("enUS"), u32(utf16.length), u32(28), utf16]);
}
const xyz = (v: number[]) => Buffer.concat([sig("XYZ "), u32(0), ...v.map(s15)]);

let cached: Buffer | undefined;

/** The profile's bytes: colorants are the P3 primaries adapted to D50; the curve is the sRGB transfer function. */
export function displayP3Profile(): Buffer {
  if (cached) return cached;
  const m = multiply(BRADFORD_D65_TO_D50, P3_TO_XYZ_D65);
  const column = (j: number) => [m[0][j], m[1][j], m[2][j]];
  // parametric curve type 3: Y = (aX + b)^g for X >= d, else cX (exactly sRGB's)
  const curve = Buffer.concat([sig("para"), u32(0), u16(3), u16(0), ...[2.4, 1 / 1.055, 0.055 / 1.055, 1 / 12.92, 0.04045].map(s15)]);
  const chad = Buffer.concat([sig("sf32"), u32(0), ...BRADFORD_D65_TO_D50.flat().map(s15)]);
  const data: [string, Buffer][] = [
    ["desc", text("Display P3")],
    ["cprt", text("No copyright, use freely")],
    ["wtpt", xyz(D50)],
    ["rXYZ", xyz(column(0))],
    ["gXYZ", xyz(column(1))],
    ["bXYZ", xyz(column(2))],
    ["rTRC", curve],
    ["chad", chad],
  ];
  const shared: Record<string, string> = { gTRC: "rTRC", bTRC: "rTRC" };
  const entries = [...data.map(([name]) => name), ...Object.keys(shared)];
  const tableSize = 4 + 12 * entries.length;
  let offset = 128 + tableSize;
  const placed: Record<string, [number, number]> = {};
  const blobs: Buffer[] = [];
  for (const [name, blob] of data) {
    placed[name] = [offset, blob.length];
    const padded = pad4(blob);
    blobs.push(padded);
    offset += padded.length;
  }
  for (const [name, target] of Object.entries(shared)) placed[name] = placed[target];
  const table = Buffer.concat([u32(entries.length), ...entries.flatMap((name) => [sig(name), u32(placed[name][0]), u32(placed[name][1])])]);
  const header = Buffer.alloc(128);
  header.writeUInt32BE(offset, 0);
  header.writeUInt32BE(0x04300000, 8); // version 4.3
  header.write("mntrRGB XYZ ", 12, "latin1");
  [2026, 1, 1, 0, 0, 0].forEach((v, i) => header.writeUInt16BE(v, 24 + i * 2));
  header.write("acsp", 36, "latin1");
  D50.forEach((v, i) => s15(v).copy(header, 68 + i * 4));
  cached = Buffer.concat([header, table, ...blobs]);
  return cached;
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf: Buffer) {
  let c = 0xffffffff;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/** Re-chunks a PNG to declare Display P3: drops any sRGB/gAMA/cHRM/iCCP chunk and adds an iCCP after IHDR. */
export function tagDisplayP3(png: Buffer): Buffer {
  const out: Buffer[] = [png.subarray(0, 8)];
  let at = 8;
  while (at < png.length) {
    const length = png.readUInt32BE(at);
    const type = png.toString("latin1", at + 4, at + 8);
    const chunk = png.subarray(at, at + 12 + length);
    at += 12 + length;
    if (["sRGB", "gAMA", "cHRM", "iCCP"].includes(type)) continue;
    out.push(chunk);
    if (type === "IHDR") {
      const body = Buffer.concat([sig("iCCP"), sig("Display P3"), Buffer.from([0, 0]), deflateSync(displayP3Profile())]);
      out.push(Buffer.concat([u32(body.length - 4), body, u32(crc32(body))]));
    }
  }
  return Buffer.concat(out);
}
