// Renders an App icon's Dials into every platform file the app ships, and checks it.
//   bun icon/kit/render.ts icon/icon.json
//   bun icon/kit/render.ts icon/icon.json --check-ios   (after `expo prebuild`, when Expo's icon is wide gamut)
// icon.json lives in <repo>/icon/; its `outputs` paths are relative to <repo>. Writes icon/icon.svg (the icon as
// macOS and Windows show it) and, per output: web favicons and PWA icons, Expo's icon and adaptive layers,
// native Android mipmaps, desktop .png and .ico. Web icons (and Expo's, when `wideGamut.expo` is on) are
// Display P3 PNGs carrying the P3 profile; everything else is sRGB.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { load } from "./deps";
import { report as alignment } from "./align";
import { tagDisplayP3 } from "./icc";
import { type Dials, type Part, type Space, build, palette } from "./icon";
import { css, toHex, wcag } from "./oklch";

const { Resvg } = load("@resvg/resvg-js");

export function png(svg: string, px: number, space: Space = "srgb"): Buffer {
  const out = new Resvg(svg, { fitTo: { mode: "width", value: px } }).render().asPng();
  return space === "p3" ? tagDisplayP3(out) : out;
}

/** Whether a PNG declares the Display P3 profile. */
export function isDisplayP3(file: Buffer): boolean {
  let at = 8;
  while (at < file.length) {
    const length = file.readUInt32BE(at);
    const type = file.toString("latin1", at + 4, at + 8);
    if (type === "iCCP") return file.toString("latin1", at + 8, at + 8 + 10) === "Display P3";
    if (type === "IDAT") return false;
    at += 12 + length;
  }
  return false;
}

/** A Windows .ico holding a PNG for each size. */
export function ico(svg: string, sizes = [16, 24, 32, 48, 64, 128, 256]): Buffer {
  const images = sizes.map((px) => png(svg, px));
  const header = Buffer.alloc(6 + 16 * sizes.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(sizes.length, 4);
  let offset = header.length;
  sizes.forEach((px, i) => {
    const at = 6 + 16 * i;
    header.writeUInt8(px >= 256 ? 0 : px, at);
    header.writeUInt8(px >= 256 ? 0 : px, at + 1);
    header.writeUInt16LE(1, at + 4);
    header.writeUInt16LE(32, at + 6);
    header.writeUInt32LE(images[i].length, at + 8);
    header.writeUInt32LE(offset, at + 12);
    offset += images[i].length;
  });
  return Buffer.concat([header, ...images]);
}

/** How far the Mark reaches from the artboard's centre, in artboard units. */
export function reach(d: Dials): number {
  const img = new Resvg(build(d, "mark"), { fitTo: { mode: "width", value: 512 } }).render();
  const { width, height, pixels } = img;
  let max = 0;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (pixels[(y * width + x) * 4 + 3] > 8) max = Math.max(max, Math.hypot(x + 0.5 - width / 2, y + 0.5 - height / 2));
    }
  }
  return (max * 1024) / width;
}

// A round launcher mask shows a 66 dp circle of the 72 dp the artboard spans: radius 469 units. A PWA maskable
// icon's safe zone is a circle of 40% of its size: radius 410.
export const SAFE = { android: 469, maskable: 410 };
export const fitFor = (r: number, safe: number) => Math.min(1, (safe / r) * 0.98);

function write(path: string, data: Buffer | string) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, data);
  console.log(`  ${path}`);
}

if (import.meta.main) {
  const file = resolve(process.argv[2] ?? "icon/icon.json");
  const d: Dials = JSON.parse(readFileSync(file, "utf8"));
  const here = dirname(file);
  const repo = dirname(here);
  const out = d.outputs ?? {};

  if (process.argv.includes("--check-ios")) {
    // Expo keeps an icon's colour profile only when it resizes with sharp; with jimp it drops it, and a P3 icon
    // would ship washed out. So after prebuild, every icon in the iOS asset catalog must still declare P3.
    const icons = [...new Bun.Glob("**/AppIcon.appiconset/*.png").scanSync({ cwd: repo, absolute: true })];
    const lost = icons.filter((file) => !isDisplayP3(readFileSync(file)));
    if (icons.length === 0) console.log("No iOS app icons found: run `bunx expo prebuild -p ios` first.");
    else if (lost.length === 0) console.log(`All ${icons.length} iOS app icons kept the Display P3 profile.`);
    else {
      console.log(`${lost.length} of ${icons.length} iOS app icons lost the Display P3 profile and would look washed out:`);
      for (const file of lost) console.log(`  ${file}`);
      console.log('Set "wideGamut": { "expo": false } in icon.json and render again, or make sharp available to prebuild.');
      process.exitCode = 1;
    }
    process.exit();
  }

  const wide = { web: d.wideGamut?.web ?? true, expo: d.wideGamut?.expo ?? false };
  const p = palette(d);
  const p3 = palette(d, "p3");
  const r = reach(d);
  const android = fitFor(r, SAFE.android);
  const maskable = fitFor(r, SAFE.maskable);
  const part = (name: Part, fit = 1, space: Space = "srgb") => build(d, name, fit, space);
  const webSpace: Space = wide.web ? "p3" : "srgb";
  const expoSpace: Space = wide.expo ? "p3" : "srgb";

  console.log(`${d.name} (${d.vibe.join(", ")})`);
  console.log(`  Tile ${css(p.top)} ${toHex(p.top)} -> ${css(p.bottom)} ${toHex(p.bottom)}`);
  console.log(`  White on the Tile: ${wcag("#FFFFFF", toHex(p.top)).toFixed(2)} at the top, ${wcag("#FFFFFF", toHex(p.bottom)).toFixed(2)} at the bottom (Expo's template: 2.71 and 4.71)`);
  console.log(`  Wide gamut (P3) Tile: ${css(p3.top)} -> ${css(p3.bottom)}, used for ${[wide.web && "web", wide.expo && "Expo"].filter(Boolean).join(" and ") || "nothing"}`);
  console.log(`  The Mark reaches ${r.toFixed(0)} from the centre: drawn at ${(android * 100).toFixed(0)}% on Android, ${(maskable * 100).toFixed(0)}% for PWA maskable`);
  if (!alignment(d) && !process.argv.includes("--force")) {
    console.log("Not writing anything: fix the Mark first (see references/marks.md, Alignment). --force writes anyway.");
    process.exit(1);
  }
  console.log("Wrote:");

  write(join(here, "icon.svg"), part("desktop"));

  if (out.web) {
    const dir = join(repo, out.web);
    write(join(dir, "favicon.ico"), ico(part("desktop"), [16, 32, 48]));
    write(join(dir, "apple-touch-icon.png"), png(part("full", 1, webSpace), 180, webSpace));
    write(join(dir, "icon-192.png"), png(part("desktop", 1, webSpace), 192, webSpace));
    write(join(dir, "icon-512.png"), png(part("desktop", 1, webSpace), 512, webSpace));
    write(join(dir, "icon-maskable-512.png"), png(part("full", maskable, webSpace), 512, webSpace));
  }
  if (out.expo) {
    const dir = join(repo, out.expo);
    write(join(dir, "icon.png"), png(part("full", 1, expoSpace), 1024, expoSpace));
    write(join(dir, "android-icon-background.png"), png(part("background"), 1024));
    write(join(dir, "android-icon-foreground.png"), png(part("foreground", android), 1024));
    write(join(dir, "android-icon-monochrome.png"), png(part("monochrome", android), 1024));
  }
  if (out.android) {
    const res = join(repo, out.android);
    const densities = { mdpi: 108, hdpi: 162, xhdpi: 216, xxhdpi: 324, xxxhdpi: 432 };
    for (const [density, px] of Object.entries(densities)) {
      write(join(res, `mipmap-${density}`, "ic_launcher_background.png"), png(part("background"), px));
      write(join(res, `mipmap-${density}`, "ic_launcher_foreground.png"), png(part("foreground", android), px));
      write(join(res, `mipmap-${density}`, "ic_launcher_monochrome.png"), png(part("monochrome", android), px));
    }
    write(
      join(res, "mipmap-anydpi", "ic_launcher.xml"),
      `<?xml version="1.0" encoding="utf-8"?>
<!-- Rendered from icon/icon.json by the solar-enterprise-app-icon kit. -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@mipmap/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
    <monochrome android:drawable="@mipmap/ic_launcher_monochrome" />
</adaptive-icon>
`,
    );
  }
  if (out.desktop) {
    const dir = join(repo, out.desktop);
    write(join(dir, "icon.png"), png(part("desktop"), 512));
    write(join(dir, "icon-512@2x.png"), png(part("desktop"), 1024));
    write(join(dir, "icon.ico"), ico(part("desktop")));
  }
}
