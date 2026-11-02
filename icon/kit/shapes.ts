// Building blocks for Marks. Everything is drawn on the 1024-unit artboard (the Tile edge to edge, centre 512, 512).
// See references/marks.md for the geometry rules these helpers default into.

export type Point = [number, number];

/** A closed path through `points`, each corner rounded by its own radius. Uneven radii are the point: big outer
 *  corners, tight inner ones. */
export function rounded(points: Point[], radii: number | number[]): string {
  const n = points.length;
  let d = "";
  for (let i = 0; i < n; i++) {
    const p0 = points[(i - 1 + n) % n];
    const p1 = points[i];
    const p2 = points[(i + 1) % n];
    const r = Array.isArray(radii) ? radii[i] : radii;
    const v0: Point = [p0[0] - p1[0], p0[1] - p1[1]];
    const v2: Point = [p2[0] - p1[0], p2[1] - p1[1]];
    const l0 = Math.hypot(...v0);
    const l2 = Math.hypot(...v2);
    const r0 = Math.min(r, l0 / 2);
    const r2 = Math.min(r, l2 / 2);
    const a: Point = [p1[0] + (v0[0] / l0) * r0, p1[1] + (v0[1] / l0) * r0];
    const b: Point = [p1[0] + (v2[0] / l2) * r2, p1[1] + (v2[1] / l2) * r2];
    d += `${i === 0 ? "M" : "L"}${f(a[0])} ${f(a[1])}Q${f(p1[0])} ${f(p1[1])} ${f(b[0])} ${f(b[1])}`;
  }
  return d + "Z";
}

/** A circle as a path. Pass `hole` for a circle cut out of a shape drawn with fill-rule evenodd. */
export function circle(cx: number, cy: number, r: number, hole = false): string {
  const s = hole ? 0 : 1;
  return `M${f(cx)} ${f(cy - r)}A${r} ${r} 0 1 ${s} ${f(cx)} ${f(cy + r)}A${r} ${r} 0 1 ${s} ${f(cx)} ${f(cy - r)}Z`;
}

/** Rotates (degrees, clockwise), scales and moves points about a centre (the artboard's, by default). */
export function place(points: Point[], { angle = 0, scale = 1, dx = 0, dy = 0, cx = 512, cy = 512 } = {}): Point[] {
  const a = (angle * Math.PI) / 180;
  const [c, s] = [Math.cos(a), Math.sin(a)];
  return points.map(([x, y]) => {
    const [u, v] = [(x - cx) * scale, (y - cy) * scale];
    return [cx + u * c - v * s + dx, cy + u * s + v * c + dy];
  });
}

/** A stroke from p0 (w0 wide) to p1 (w1 wide) with its own end radii: the Taper as a building block.
 *  Keep w1 15–25% thinner than w0. */
export function tapered(p0: Point, p1: Point, w0: number, w1: number, r0: number, r1: number): string {
  const [dx, dy] = [p1[0] - p0[0], p1[1] - p0[1]];
  const L = Math.hypot(dx, dy);
  const [nx, ny] = [-dy / L, dx / L];
  return rounded(
    [
      [p0[0] + (nx * w0) / 2, p0[1] + (ny * w0) / 2],
      [p1[0] + (nx * w1) / 2, p1[1] + (ny * w1) / 2],
      [p1[0] - (nx * w1) / 2, p1[1] - (ny * w1) / 2],
      [p0[0] - (nx * w0) / 2, p0[1] - (ny * w0) / 2],
    ],
    [r0, r1, r1, r0],
  );
}

/** Where two lines meet: p1–p2 and p3–p4, extended. Builds outlines from offset edges (see examples/test-apps). */
export function meet(p1: Point, p2: Point, p3: Point, p4: Point): Point {
  const [d1x, d1y, d2x, d2y] = [p2[0] - p1[0], p2[1] - p1[1], p4[0] - p3[0], p4[1] - p3[1]];
  const t = ((p3[0] - p1[0]) * d2y - (p3[1] - p1[1]) * d2x) / (d1x * d2y - d1y * d2x);
  return [p1[0] + d1x * t, p1[1] + d1y * t];
}

/** Points along a circle's arc from `from` to `to`, going whichever way round keeps the arc's middle where
 *  `keep` says (for example, outside another circle when cutting a crescent). Pass them to `rounded` with radius
 *  0 along the arc and real radii only at the true corners. */
export function arc(c: Point, r: number, from: Point, to: Point, keep: (middle: Point) => boolean, steps = 48): Point[] {
  const a0 = Math.atan2(from[1] - c[1], from[0] - c[0]);
  let span = Math.atan2(to[1] - c[1], to[0] - c[0]) - a0;
  if (span < 0) span += 2 * Math.PI;
  const at = (a: number): Point => [c[0] + r * Math.cos(a), c[1] + r * Math.sin(a)];
  if (!keep(at(a0 + span / 2))) span -= 2 * Math.PI;
  return Array.from({ length: steps }, (_, i) => at(a0 + (span * i) / steps));
}

/** Points along a quadratic curve (p0, control c, p1), from parameter `from` to `to`. Use the same p0, c, p1 the
 *  outline is drawn with, so anything built on the curve lines up with the outline exactly. */
export function quad(p0: Point, c: Point, p1: Point, from = 0, to = 1, steps = 32): Point[] {
  return Array.from({ length: steps + 1 }, (_, i) => {
    const t = from + ((to - from) * i) / steps;
    return [(1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * c[0] + t * t * p1[0], (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * c[1] + t * t * p1[1]];
  });
}

/** The true midline between two sides of a shape (each a line of points, both running the same way): every point
 *  sits halfway between the sides, measured across the midline itself. On a lopsided shape this is not the
 *  average of the sides' control points, which lands off-centre wherever one side bulges more. `from`/`to` trim
 *  the ends (0 = the sides' start, 1 = their end). */
export function between(sideA: Point[], sideB: Point[], from = 0, to = 1, steps = 32): Point[] {
  const at = (side: Point[], t: number): Point => {
    const f = t * (side.length - 1);
    const i = Math.min(side.length - 2, Math.floor(f));
    const k = f - i;
    return [side[i][0] + (side[i + 1][0] - side[i][0]) * k, side[i][1] + (side[i + 1][1] - side[i][1]) * k];
  };
  const hit = (side: Point[], p: Point, n: Point): Point | undefined => {
    let best: Point | undefined;
    for (let i = 0; i < side.length - 1; i++) {
      const [a, b] = [side[i], side[i + 1]];
      const [ex, ey] = [b[0] - a[0], b[1] - a[1]];
      const den = n[0] * ey - n[1] * ex;
      if (Math.abs(den) < 1e-9) continue;
      const s = ((a[0] - p[0]) * ey - (a[1] - p[1]) * ex) / den;
      const u = ((a[0] - p[0]) * n[1] - (a[1] - p[1]) * n[0]) / den;
      if (u < 0 || u > 1) continue;
      const q: Point = [p[0] + n[0] * s, p[1] + n[1] * s];
      if (!best || Math.abs(s) < Math.hypot(best[0] - p[0], best[1] - p[1])) best = q;
    }
    return best;
  };
  // Start from the smooth average of the sides, then move each point across it to halfway between the sides.
  // The normals come from the smooth start and are not recomputed, so the result stays smooth (recomputing them
  // from moved neighbours amplifies small differences into kinks).
  const start: Point[] = Array.from({ length: steps + 1 }, (_, i) => {
    const t = from + ((to - from) * i) / steps;
    const [a, b] = [at(sideA, t), at(sideB, t)];
    return [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
  });
  return start.map((p, i) => {
    const [a, b] = [start[Math.max(0, i - 1)], start[Math.min(start.length - 1, i + 1)]];
    const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
    const n: Point = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
    const [ha, hb] = [hit(sideA, p, n), hit(sideB, p, n)];
    return ha && hb ? [(ha[0] + hb[0]) / 2, (ha[1] + hb[1]) / 2] : p;
  });
}

/** A stroke that follows a line of points (straight or curved), tapering from w0 wide at its start to w1 at its
 *  end, with end corners r0 and r1. This is how inner features (veins, slots) follow the outline they sit in. */
export function band(line: Point[], w0: number, w1: number, r0: number, r1: number): string {
  const n = line.length;
  const sides = line.map((p, i) => {
    const [a, b] = [line[Math.max(0, i - 1)], line[Math.min(n - 1, i + 1)]];
    const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
    const [nx, ny] = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
    const half = (w0 + ((w1 - w0) * i) / (n - 1)) / 2;
    return [[p[0] + nx * half, p[1] + ny * half], [p[0] - nx * half, p[1] - ny * half]] as [Point, Point];
  });
  const outline = [...sides.map((s) => s[0]), ...sides.map((s) => s[1]).reverse()];
  const radii = outline.map((_, i) => (i === 0 || i === outline.length - 1 ? r0 : i === n - 1 || i === n ? r1 : 0));
  return rounded(outline, radii);
}

/** Expo's template chevron, measured row by row off the original 1024 icon: a broad flat cap, legs that taper
 *  from 170 to 136, inner edges steeper than outer ones, ends cut on a slant, a tight notch. The reference for
 *  every ratio in references/marks.md. */
export const CHEVRON: { points: Point[]; radii: number[] } = {
  points: [[446, 186], [578, 186], [848, 712], [754, 800], [512, 386], [270, 800], [176, 712]],
  radii: [54, 54, 66, 44, 16, 44, 66],
};

/** The chevron's outline without its notch: the outside of a triangle that keeps the chevron's cap, slanted
 *  corners and uneven radii. */
export const CHEVRON_OUTLINE: { points: Point[]; radii: number[] } = {
  points: [[446, 186], [578, 186], [848, 712], [754, 800], [270, 800], [176, 712]],
  radii: [54, 54, 66, 44, 44, 66],
};

const f = (v: number) => +v.toFixed(1);
