// A review sheet for a Variant round: each variant large, as a round Android launcher icon, and at 48 and 24 px on
// a light and a dark background, side by side.
//   bun kit/preview.ts sheet.png a.json b.json c.json d.json
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";
import { load } from "./deps";
import { type Dials, build } from "./icon";
import { report as alignment } from "./align";
import { SAFE, fitFor, png, reach } from "./render";

const { Resvg } = load("@resvg/resvg-js");

const [out, ...files] = process.argv.slice(2);
if (!out || files.length === 0) {
  console.error("usage: bun kit/preview.ts <sheet.png> <icon.json>...");
  process.exit(1);
}

const uri = (svg: string, px: number) => `data:image/png;base64,${png(svg, px).toString("base64")}`;
const COL = 340;
let body = "";
files.forEach((file, i) => {
  const d: Dials = JSON.parse(readFileSync(file, "utf8"));
  console.log(`${String.fromCharCode(65 + i)} · ${d.name || basename(file)}`);
  alignment(d);
  const fit = fitFor(reach(d), SAFE.android);
  const x = 20 + i * COL;
  const big = uri(build(d, "desktop"), 300);
  const bg = uri(build(d, "background"), 240);
  const fg = uri(build(d, "foreground", fit), 240);
  const small = (px: number) => uri(build(d, "desktop"), px);
  body += `
    <text x="${x}" y="30" font-family="Segoe UI, Inter, Helvetica, sans-serif" font-size="18" fill="#333">${String.fromCharCode(65 + i)} · ${d.name || basename(file)}</text>
    <image x="${x}" y="40" width="300" height="300" href="${big}"/>
    <g clip-path="url(#round${i})">
      <image x="${x}" y="350" width="240" height="240" href="${bg}"/>
      <image x="${x}" y="350" width="240" height="240" href="${fg}"/>
    </g>
    <clipPath id="round${i}"><circle cx="${x + 120}" cy="470" r="80"/></clipPath>
    <rect x="${x}" y="600" width="300" height="80" fill="#FAFAFA"/>
    <rect x="${x}" y="680" width="300" height="80" fill="#1C1C1E"/>
    <image x="${x + 30}" y="616" width="48" height="48" href="${small(48)}"/>
    <image x="${x + 110}" y="628" width="24" height="24" href="${small(24)}"/>
    <image x="${x + 30}" y="696" width="48" height="48" href="${small(48)}"/>
    <image x="${x + 110}" y="708" width="24" height="24" href="${small(24)}"/>`;
});
const width = 20 + files.length * COL;
const sheet = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="780" viewBox="0 0 ${width} 780">
  <rect width="${width}" height="780" fill="#E8E8EC"/>${body}
</svg>`;
writeFileSync(out, new Resvg(sheet, { font: { loadSystemFonts: true } }).render().asPng());
console.log(`Wrote ${out}`);
