// Measures a reference icon's Mark row by row, for a Calibration copy: where the Mark starts and ends on each
// row, and how wide each run is. Tapers, caps and slanted ends show up as the numbers change.
//   bun kit/measure.ts reference.png [step=20] [threshold]
// The Mark is whatever is lighter than `threshold` (0–255 lightness). By default it is halfway between the
// Tile's corner and the lightest pixel.
import { readFileSync } from "node:fs";
import { load } from "./deps";

const { PNG } = load("pngjs");
const [file, stepArg, thresholdArg] = process.argv.slice(2);
if (!file) {
  console.error("usage: bun kit/measure.ts <reference.png> [step] [threshold]");
  process.exit(1);
}
const img = PNG.sync.read(readFileSync(file));
const { width, height, data } = img;
const light = (x: number, y: number) => {
  const i = (y * width + x) * 4;
  return 0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2];
};
let brightest = 0;
for (let y = 0; y < height; y += 2) for (let x = 0; x < width; x += 2) brightest = Math.max(brightest, light(x, y));
// A Tile's gradient and a Mark's Frost both darken downward, so each row is judged against its own background
// (its left edge, inside the margin): the Mark is whatever rises a third of the way from there to the lightest.
const rowThreshold = (y: number) => {
  if (thresholdArg) return +thresholdArg;
  const ground = light(Math.floor(width * 0.06), y);
  return ground + (brightest - ground) * 0.33;
};
const step = +(stepArg ?? 20);
const scale = 1024 / width;

console.log(`${file}: ${width}×${height}; numbers on the 1024 artboard: start–end (width) of each run`);
for (let y = 0; y < height; y += Math.max(1, Math.round(step / scale))) {
  const runs: [number, number][] = [];
  const threshold = rowThreshold(y);
  let start = -1;
  for (let x = 0; x <= width; x++) {
    const inside = x < width && light(x, y) > threshold;
    if (inside && start < 0) start = x;
    if (!inside && start >= 0) {
      if (x - start > 3) runs.push([start, x - 1]);
      start = -1;
    }
  }
  if (runs.length) {
    const fmt = runs.map(([a, b]) => `${Math.round(a * scale)}–${Math.round(b * scale)} (${Math.round((b - a) * scale)})`);
    console.log(`  y ${String(Math.round(y * scale)).padStart(4)}: ${fmt.join("   ")}`);
  }
}
