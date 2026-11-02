// Checks that a Mark's inner features sit true in their shape. Any long cut-out (a vein, a slot, a line) is
// measured along its length: how thick the wall is on each side of it, from one end to the other. A feature
// that follows the shape keeps the same balance all the way along (centred, or offset on purpose by the same
// amount); one that drifts, wider on one side at one end and on the other side at the other, is misaligned.
//   bun kit/align.ts icon/icon.json
import { readFileSync } from "node:fs";
import { load } from "./deps";
import { type Dials, build } from "./icon";

const SIZE = 1024;
/** How far the wall balance may wander along a feature (0.5 is centred) before it counts as misaligned. A feature
 *  built on the shape's true midline stays within 0.02; one on the average of the sides' control points drifts
 *  0.06 on a lopsided leaf, and that shows. */
export const MAX_DRIFT = 0.04;
/** The sharpest bend (artboard units of sideways jump between neighbouring samples) a feature's centreline may
 *  take before it counts as jagged. A smooth curve stays well under 1. */
export const MAX_KINK = 2.5;

export type Feature = {
  at: [number, number];
  length: number;
  balance: number[];
  drift: number;
  kink: number;
  ok: boolean;
};

function masks(d: Dials) {
  const { Resvg } = load("@resvg/resvg-js");
  const { pixels } = new Resvg(build(d, "mark"), { fitTo: { mode: "width", value: SIZE } }).render();
  const mark = new Uint8Array(SIZE * SIZE);
  for (let i = 0; i < mark.length; i++) mark[i] = pixels[i * 4 + 3] > 127 ? 1 : 0;
  // Everything the outside can't reach without crossing the Mark is inside its silhouette.
  const outside = new Uint8Array(SIZE * SIZE);
  const queue: number[] = [];
  for (let i = 0; i < SIZE; i++) queue.push(i, (SIZE - 1) * SIZE + i, i * SIZE, i * SIZE + SIZE - 1);
  while (queue.length) {
    const i = queue.pop()!;
    if (outside[i] || mark[i]) continue;
    outside[i] = 1;
    const [x, y] = [i % SIZE, Math.floor(i / SIZE)];
    if (x > 0) queue.push(i - 1);
    if (x < SIZE - 1) queue.push(i + 1);
    if (y > 0) queue.push(i - SIZE);
    if (y < SIZE - 1) queue.push(i + SIZE);
  }
  return { mark, outside };
}

/** The cut-outs: connected areas inside the silhouette that the Mark doesn't cover. */
function holes(mark: Uint8Array, outside: Uint8Array) {
  const seen = new Uint8Array(SIZE * SIZE);
  const found: number[][] = [];
  for (let start = 0; start < SIZE * SIZE; start++) {
    if (seen[start] || mark[start] || outside[start]) continue;
    const pixels: number[] = [];
    const queue = [start];
    seen[start] = 1;
    while (queue.length) {
      const i = queue.pop()!;
      pixels.push(i);
      for (const j of [i - 1, i + 1, i - SIZE, i + SIZE]) {
        if (j >= 0 && j < SIZE * SIZE && !seen[j] && !mark[j] && !outside[j]) {
          seen[j] = 1;
          queue.push(j);
        }
      }
    }
    if (pixels.length > 60) found.push(pixels);
  }
  return found;
}

/** Centre, long axis and spread of a set of pixels (principal component analysis). */
function axis(pixels: number[]) {
  let [cx, cy] = [0, 0];
  for (const i of pixels) [cx, cy] = [cx + (i % SIZE), cy + Math.floor(i / SIZE)];
  [cx, cy] = [cx / pixels.length, cy / pixels.length];
  let [xx, yy, xy] = [0, 0, 0];
  for (const i of pixels) {
    const [dx, dy] = [(i % SIZE) - cx, Math.floor(i / SIZE) - cy];
    [xx, yy, xy] = [xx + dx * dx, yy + dy * dy, xy + dx * dy];
  }
  const angle = Math.atan2(2 * xy, xx - yy) / 2;
  const u: [number, number] = [Math.cos(angle), Math.sin(angle)];
  let [lo, hi, spread] = [Infinity, -Infinity, 0];
  for (const i of pixels) {
    const [dx, dy] = [(i % SIZE) - cx, Math.floor(i / SIZE) - cy];
    const along = dx * u[0] + dy * u[1];
    [lo, hi] = [Math.min(lo, along), Math.max(hi, along)];
    spread += (dx * u[1] - dy * u[0]) ** 2;
  }
  return { cx, cy, u, lo, hi, width: 2 * Math.sqrt(spread / pixels.length) * 1.7 };
}

/** Wall thickness on each side of a cut-out, measured across its axis near (x, y). A curved cut-out can bend
 *  away from its straight axis, so first find the part of it closest to the axis, then walk out of it both ways. */
function walls(mark: Uint8Array, hole: Set<number>, x: number, y: number, n: [number, number]): [number, number, number] {
  const index = (s: number) => {
    const [px, py] = [Math.round(x + n[0] * s), Math.round(y + n[1] * s)];
    return px < 0 || py < 0 || px >= SIZE || py >= SIZE ? -1 : py * SIZE + px;
  };
  let centre: number | undefined;
  for (let k = 0; k <= 80 && centre === undefined; k++) {
    if (hole.has(index(k))) centre = k;
    else if (hole.has(index(-k))) centre = -k;
  }
  if (centre === undefined) return [0, 0, 0];
  const across = (dir: number) => {
    let s = centre!;
    while (hole.has(index(s))) s += dir;
    return s;
  };
  const side = (dir: number) => {
    let s = across(dir);
    const edge = s;
    while (index(s) >= 0 && mark[index(s)] === 1) s += dir;
    return Math.abs(s - edge);
  };
  // The third value is where the feature's own middle sits across the axis, to follow its centreline.
  return [side(1), side(-1), (across(1) + across(-1)) / 2];
}

export function features(d: Dials): Feature[] {
  const { mark, outside } = masks(d);
  const scale = 1024 / SIZE;
  return holes(mark, outside)
    .map((pixels) => ({ pixels, ...axis(pixels) }))
    .filter((h) => (h.hi - h.lo) / h.width > 3) // only long, thin cut-outs have an axis to keep true
    .map((h) => {
      const n: [number, number] = [-h.u[1], h.u[0]];
      const hole = new Set(h.pixels);
      const sample = (t: number) => {
        const s = h.lo + (h.hi - h.lo) * t;
        return walls(mark, hole, h.cx + h.u[0] * s, h.cy + h.u[1] * s, n);
      };
      const balance = [0.15, 0.3, 0.5, 0.7, 0.85].map((t) => {
        const [a, b] = sample(t);
        return a + b ? a / (a + b) : 0.5;
      });
      const drift = Math.max(...balance) - Math.min(...balance);
      // Smoothness: the centreline's sideways position at 33 points; a kink is a sudden change in its slope.
      const centre = Array.from({ length: 33 }, (_, i) => sample(0.1 + (0.8 * i) / 32)[2]);
      const kink = Math.max(0, ...centre.slice(1, -1).map((c, i) => Math.abs(centre[i] - 2 * c + centre[i + 2]))) * scale;
      const at: [number, number] = [h.cx * scale, h.cy * scale];
      return { at, length: (h.hi - h.lo) * scale, balance, drift, kink, ok: drift <= MAX_DRIFT && kink <= MAX_KINK };
    });
}

export function report(d: Dials): boolean {
  const found = features(d);
  if (found.length === 0) {
    console.log("  Alignment: no long cut-outs to check");
    return true;
  }
  for (const f of found) {
    const where = `cut-out at ${f.at.map((v) => v.toFixed(0)).join(", ")} (${f.length.toFixed(0)} long)`;
    const walls = f.balance.map((b) => b.toFixed(2)).join(" ");
    const facts = `balance ${walls}, drift ${f.drift.toFixed(2)}, sharpest bend ${f.kink.toFixed(1)}`;
    if (f.ok) console.log(`  Alignment: ${where} sits true and runs smooth (${facts})`);
    if (f.drift > MAX_DRIFT) {
      console.log(
        `  MISALIGNED: ${where}: its walls change along its length (${facts}; limit ${MAX_DRIFT}). ` +
          "Build it on the outline's true midline (shapes.ts `between`) or its edges instead of placing it.",
      );
    }
    if (f.kink > MAX_KINK) {
      console.log(
        `  JAGGED: ${where}: its centreline kinks (${facts}; limit ${MAX_KINK}). ` +
          "Build it from a smooth line: fixed normals, enough points, no per-point corrections that feed back.",
      );
    }
  }
  return found.every((f) => f.ok);
}

if (import.meta.main) {
  const ok = report(JSON.parse(readFileSync(process.argv[2] ?? "icon/icon.json", "utf8")));
  process.exitCode = ok ? 0 : 1;
}
