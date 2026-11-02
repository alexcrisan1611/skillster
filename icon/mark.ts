// Skillster's Mark: one stem (the source folder) flaring into three curved, tapering branches (the tools it links
// into). Every wall is offset from its stroke's own centre curve; corners sit where neighbouring walls cross.
//   bun icon/mark.ts   (writes the path into icon/icon.json; then bun icon/kit/render.ts icon/icon.json)
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { type Point, rounded } from "./kit/shapes";

const add = (a: Point, b: Point): Point => [a[0] + b[0], a[1] + b[1]];
const sub = (a: Point, b: Point): Point => [a[0] - b[0], a[1] - b[1]];
const mul = (a: Point, k: number): Point => [a[0] * k, a[1] * k];
const unit = (a: Point): Point => mul(a, 1 / Math.hypot(...a));
const side = (d: Point): Point => unit([d[1], -d[0]]); // left of the direction of travel

/** A stroke along a quadratic curve, w0 wide at p0 and w1 at p1. */
type Stroke = { p0: Point; c: Point; p1: Point; w0: number; w1: number };
const STEPS = 64;
/** One wall (k = +1 left, -1 right) sampled from the stroke's own curve, run on past each end along its tangent. */
function wall(s: Stroke, k: 1 | -1, reach = 0): Point[] {
  const pts: Point[] = [];
  const tan = (t: number): Point => unit(add(mul(sub(s.c, s.p0), 2 * (1 - t)), mul(sub(s.p1, s.c), 2 * t)));
  for (let i = 0; i <= STEPS; i++) {
    const t = i / STEPS;
    const p: Point = [(1 - t) ** 2 * s.p0[0] + 2 * (1 - t) * t * s.c[0] + t * t * s.p1[0], (1 - t) ** 2 * s.p0[1] + 2 * (1 - t) * t * s.c[1] + t * t * s.p1[1]];
    pts.push(add(p, mul(side(tan(t)), (k * (s.w0 + (s.w1 - s.w0) * t)) / 2)));
  }
  if (reach) {
    pts.unshift(add(pts[0], mul(tan(0), -reach)));
    pts.push(add(pts[pts.length - 1], mul(tan(1), reach)));
  }
  return pts;
}
/** Where two walls cross: the point, and the segment index on each. */
function cross(a: Point[], b: Point[]): { p: Point; i: number; j: number } {
  for (let i = 0; i < a.length - 1; i++)
    for (let j = 0; j < b.length - 1; j++) {
      const [p, r] = [a[i], sub(a[i + 1], a[i])], [q, s] = [b[j], sub(b[j + 1], b[j])];
      const den = r[0] * s[1] - r[1] * s[0];
      if (Math.abs(den) < 1e-9) continue;
      const t = ((q[0] - p[0]) * s[1] - (q[1] - p[1]) * s[0]) / den;
      const u = ((q[0] - p[0]) * r[1] - (q[1] - p[1]) * r[0]) / den;
      if (t >= 0 && t <= 1 && u >= 0 && u <= 1) return { p: add(p, mul(r, t)), i, j };
    }
  throw new Error("walls don't cross");
}
type Corner = { p: Point; r: number };
/** Rounds the true corners; curve points too close to a corner are dropped so the corner gets its full radius. */
function outline(cs: Corner[]): string {
  const keep = cs.filter((c, i) => c.r > 0 || !cs.some((o) => o.r > 0 && Math.hypot(...sub(o.p, c.p)) < o.r * 2.1));
  return rounded(keep.map((c) => c.p), keep.map((c) => c.r));
}
const curve = (pts: Point[]): Corner[] => pts.map((p) => ({ p, r: 0 }));
const tipDir = (w: Point[]) => unit(sub(w[w.length - 1], w[w.length - 2]));

type Fan = { trunk: Stroke; branches: [Stroke, Stroke, Stroke]; slant: number[]; radii: { foot: [number, number]; joint: [number, number]; tips: number[]; crotch: [number, number] } };

function fan({ trunk, branches, slant, radii }: Fan): string {
  const R = 160;
  const tl = wall(trunk, 1, R), tr = wall(trunk, -1, R);
  const ws = branches.map((b) => [wall(b, 1, R), wall(b, -1, R)] as [Point[], Point[]]);
  // Crossings, walking round clockwise: trunk-left/L-left, L-right/M-left, M-right/R-left, R-right/trunk-right.
  const x0 = cross(tl, ws[0][0]), x1 = cross(ws[0][1], ws[1][0]), x2 = cross(ws[1][1], ws[2][0]), x3 = cross(ws[2][1], tr);
  const end = (w: Point[], s: number) => add(w[w.length - 2], mul(tipDir(w), s)); // the wall's end (before the run-on), slanted
  const cs: Corner[] = [];
  cs.push({ p: add(tl[1], mul(unit(sub(tl[2], tl[1])), -slant[0])), r: radii.foot[0] });
  cs.push(...curve(tl.slice(2, x0.i + 1)), { p: x0.p, r: radii.joint[0] });
  const xs = [x1, x2];
  ws.forEach(([left, right], b) => {
    const from = b === 0 ? x0.j + 1 : xs[b - 1].j + 1;
    cs.push(...curve(left.slice(from, left.length - 2)));
    cs.push({ p: end(left, slant[1 + b * 2]), r: radii.tips[b * 2] });
    cs.push({ p: end(right, slant[2 + b * 2]), r: radii.tips[b * 2 + 1] });
    const to = b < 2 ? xs[b].i + 1 : x3.i + 1;
    cs.push(...curve(right.slice(to, right.length - 2).reverse()));
    if (b < 2) cs.push({ p: xs[b].p, r: radii.crotch[b] });
  });
  cs.push({ p: x3.p, r: radii.joint[1] });
  cs.push(...curve(tr.slice(2, x3.j + 1).reverse()));
  cs.push({ p: add(tr[1], mul(unit(sub(tr[2], tr[1])), -slant[7])), r: radii.foot[1] });
  return outline(cs);
}

// Skillster's numbers. The stem leans a touch; the branches start steep and flare out, each its own length.
const SKILLSTER: Fan = {
  trunk: { p0: [498, 852], c: [494, 720], p1: [512, 600], w0: 120, w1: 150 },
  branches: [
    { p0: [512, 600], c: [438, 452], p1: [238, 320], w0: 122, w1: 96 },
    { p0: [512, 600], c: [530, 420], p1: [548, 208], w0: 118, w1: 92 },
    { p0: [512, 600], c: [592, 454], p1: [794, 340], w0: 120, w1: 94 },
  ],
  // foot (left), L tip left/right, M tip left/right, R tip left/right, foot (right): the inner side reaches further
  slant: [14, -10, 18, 14, -8, 18, -10, -10],
  radii: { foot: [30, 26], joint: [64, 58], tips: [40, 32, 38, 42, 32, 40], crotch: [12, 12] },
};

const file = join(import.meta.dir, "icon.json");
const dials = JSON.parse(readFileSync(file, "utf8"));
dials.mark.path = fan(SKILLSTER);
dials.grid.center = SKILLSTER.trunk.p1; // the Ring grid radiates from where the stem splits
writeFileSync(file, JSON.stringify(dials, null, 2) + "\n");
console.log(`Wrote the Mark into ${file}`);
