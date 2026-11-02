// Builds an App icon in the House style from its Dials (an icon.json). Everything is drawn on a 1024-unit
// artboard: the Tile edge to edge. Each "part" is one SVG the renderer turns into platform files.
import { load } from "./deps";
import { type Oklch, css, parse, toHex, toHexP3, vivid, widen } from "./oklch";

export type GridKind = "line" | "dot" | "ring";
export type AnchorShape = "dot" | "ring" | "square" | "diamond" | "plus";

export type Dials = {
  name: string;
  /** How the app should feel, in three words. The hue, Grid kind and Anchor shape are read from it. */
  vibe: string[];
  /** The Tile's hue in oklch degrees. The recipe derives every colour from it. */
  hue: number;
  /** The Mark: one path on the artboard, drawn with fill-rule evenodd (so holes cut through). */
  mark: { path: string; note?: string };
  grid: { kind: GridKind; pitch?: number; every?: number; center?: [number, number] };
  anchor: { shape: AnchorShape };
  /** Optional: exact colours instead of the recipe's ("oklch(L C H)" or "#hex"). */
  tile?: { top: string; bottom: string };
  frost?: [string, string, string, string];
  shadow?: string;
  /** Where platform files go, relative to the repo (the folder that holds icon/). Leave out what the app lacks. */
  outputs?: { web?: string; expo?: string; android?: string; desktop?: string };
  /** Which outputs are written in Display P3 (default: web yes, Expo no; see references/platforms.md). */
  wideGamut?: { web?: boolean; expo?: boolean };
};

export type Part = "full" | "desktop" | "background" | "foreground" | "monochrome" | "mark";

/** sRGB, or Display P3 for files tagged with the P3 profile (see icc.ts and render.ts). */
export type Space = "srgb" | "p3";

export type Palette = { top: Oklch; bottom: Oklch; frost: Oklch[]; shadow: Oklch };

/** The House style's colour recipe, after Expo's template icon: the Tile steps from L 0.695 to L 0.561 at 90% of
 *  the chroma ceiling, drifting a few degrees as it darkens (toward red on warm hues, toward violet on cool ones);
 *  the Mark's Frost is a near-white that settles into a pale tint of the same hue; the shadow is a deep tint,
 *  never black. */
export function palette(d: Dials, space: Space = "srgb"): Palette {
  const h = d.hue;
  const warm = h < 120 || h > 330;
  const top = d.tile ? parse(d.tile.top) : vivid(0.695, h, 0.9);
  const bottom = d.tile ? parse(d.tile.bottom) : vivid(0.561, h + (warm ? -6 : 5), 0.9);
  // On warm hues the Frost leans a little yellow, so it reads as warm white rather than pink.
  const fh = top.h + (warm ? 8 : 0);
  const frost = d.frost
    ? d.frost.map(parse)
    : [vivid(0.985, fh + 5, 0.3), vivid(0.955, fh + 3, 0.4), vivid(0.895, fh, 0.5), vivid(0.85, fh - 2, 0.55)];
  const shadow = d.shadow ? parse(d.shadow) : vivid(0.4, bottom.h - 3, 0.7);
  if (space === "srgb") return { top, bottom, frost, shadow };
  // Wide gamut: the Tile gains what chroma P3 allows at the same lightness and hue, held to what an sRGB screen
  // (which clips it) still shows as the designed colour. The Frost and shadow keep their colours.
  return { top: widen(top), bottom: widen(bottom), frost, shadow };
}

/** The Mark's bounding box on the artboard. */
export function markBounds(path: string): { x: number; y: number; width: number; height: number } {
  const { Resvg } = load("@resvg/resvg-js");
  const b = new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><path d="${path}"/></svg>`).getBBox();
  if (!b) throw new Error("the Mark's path draws nothing");
  return { x: b.x, y: b.y, width: b.width, height: b.height };
}

const WHITE = 'fill="#FFFFFF"';

function anchor(shape: AnchorShape, x: number, y: number): string {
  switch (shape) {
    case "dot":
      return `<circle cx="${x}" cy="${y}" r="6" ${WHITE}/>`;
    case "ring":
      return `<circle cx="${x}" cy="${y}" r="7" fill="none" stroke="#FFFFFF" stroke-width="2.6"/>`;
    case "square":
      return `<rect x="${x - 6}" y="${y - 6}" width="12" height="12" rx="3" ${WHITE}/>`;
    case "diamond":
      return `<rect x="${x - 5.5}" y="${y - 5.5}" width="11" height="11" rx="2.5" transform="rotate(45 ${x} ${y})" ${WHITE}/>`;
    case "plus":
      return `<path d="M${x - 16} ${y}H${x + 16}M${x} ${y - 16}V${y + 16}" stroke="#FFFFFF" stroke-width="3" stroke-linecap="round"/>`;
  }
}

/** The Grid and its Anchors, before the edge fade. Hairlines sit every `pitch`; every `every`-th line passes
 *  through Anchors and is a slightly heavier Major line. */
function grid(d: Dials): string {
  const pitch = d.grid.pitch ?? 34;
  const every = d.grid.every ?? 4;
  const count = Math.floor(1024 / pitch);
  const isMajorX = (i: number) => i % every === 3 % every;
  const isMajorY = (i: number) => i % every === 1 % every;
  const anchors: string[] = [];
  const shape = d.anchor.shape;
  const anchorOpacity = shape === "plus" ? 0.5 : 0.62;

  if (d.grid.kind === "ring") {
    const [cx, cy] = d.grid.center ?? [512, 512];
    const thin: string[] = [];
    const major: string[] = [];
    for (let k = 1; k <= 21; k++) {
      (k % every === 0 ? major : thin).push(`<circle cx="${cx}" cy="${cy}" r="${k * pitch}"/>`);
    }
    for (let a = 0; a < 360; a += 15) {
      const t = (a * Math.PI) / 180;
      thin.push(`<path d="M${cx} ${cy}L${(cx + 900 * Math.cos(t)).toFixed(0)} ${(cy + 900 * Math.sin(t)).toFixed(0)}"/>`);
      if (a % 45 === 0) {
        for (let k = every; k <= 16; k += every) {
          anchors.push(anchor(shape, +(cx + k * pitch * Math.cos(t)).toFixed(1), +(cy + k * pitch * Math.sin(t)).toFixed(1)));
        }
      }
    }
    return (
      `<g fill="none" stroke="#FFFFFF" stroke-opacity=".16" stroke-width="2">${thin.join("")}</g>` +
      `<g fill="none" stroke="#FFFFFF" stroke-opacity=".24" stroke-width="2.8">${major.join("")}</g>` +
      `<g opacity="${anchorOpacity}">${anchors.join("")}</g>`
    );
  }

  const thin: string[] = [];
  const major: string[] = [];
  const dots: string[] = [];
  for (let i = 1; i <= count; i++) {
    const v = i * pitch;
    (isMajorX(i) ? major : thin).push(`<path d="M${v} 0V1024"/>`);
    (isMajorY(i) ? major : thin).push(`<path d="M0 ${v}H1024"/>`);
  }
  for (let i = 1; i <= count; i++) {
    for (let j = 1; j <= count; j++) {
      const [x, y] = [i * pitch, j * pitch];
      if (isMajorX(i) && isMajorY(j)) anchors.push(anchor(shape, x, y));
      else if (d.grid.kind === "dot") dots.push(`<circle cx="${x}" cy="${y}" r="2.2"/>`);
    }
  }
  if (d.grid.kind === "line") {
    return (
      `<g stroke="#FFFFFF" stroke-opacity=".17" stroke-width="2">${thin.join("")}</g>` +
      `<g stroke="#FFFFFF" stroke-opacity=".24" stroke-width="2.8">${major.join("")}</g>` +
      `<g opacity="${anchorOpacity}">${anchors.join("")}</g>`
    );
  }
  // Dot grid: no lines but faint hairlines on the Major lines, so the structure still reads.
  return (
    `<g stroke="#FFFFFF" stroke-opacity=".13" stroke-width="2">${major.join("")}</g>` +
    `<g fill="#FFFFFF" fill-opacity=".34">${dots.join("")}</g>` +
    `<g opacity="${anchorOpacity}">${anchors.join("")}</g>`
  );
}

/** One part of the icon as SVG. `fit` shrinks the Mark about the centre (for round masks). */
export function build(d: Dials, part: Part, fit = 1, space: Space = "srgb"): string {
  const p = palette(d, space);
  const b = markBounds(d.mark.path);
  const hex = (o: Oklch) => (space === "p3" ? toHexP3(o) : toHex(o));
  const [sr, sg, sb] = [1, 3, 5].map((i) => (parseInt(hex(p.shadow).slice(i, i + 2), 16) / 255).toFixed(3));

  const defs = `
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1024" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="${hex(p.top)}"/><stop offset="1" stop-color="${hex(p.bottom)}"/>
    </linearGradient>
    <radialGradient id="fadeout" cx="512" cy="512" r="560" gradientUnits="userSpaceOnUse">
      <stop offset=".25" stop-color="#FFFFFF"/><stop offset="1" stop-color="#000000"/>
    </radialGradient>
    <mask id="vignette" maskUnits="userSpaceOnUse" x="-256" y="-256" width="1536" height="1536">
      <rect x="-256" y="-256" width="1536" height="1536" fill="url(#fadeout)"/>
    </mask>
    <path id="mark" fill-rule="evenodd" d="${d.mark.path}"/>
    <linearGradient id="frost" x1="0" y1="${b.y.toFixed(0)}" x2="0" y2="${(b.y + b.height).toFixed(0)}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="${hex(p.frost[0])}"/><stop offset=".1" stop-color="${hex(p.frost[1])}"/>
      <stop offset=".3" stop-color="${hex(p.frost[2])}"/><stop offset="1" stop-color="${hex(p.frost[3])}"/>
    </linearGradient>
    <filter id="rim" x="-10%" y="-10%" width="120%" height="120%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="13" result="blur"/>
      <feOffset in="blur" dy="10" result="low"/>
      <feComposite in="SourceAlpha" in2="low" operator="arithmetic" k2="1" k3="-1" result="edge"/>
      <feFlood flood-color="#FFFFFF" flood-opacity=".9"/>
      <feComposite in2="edge" operator="in"/>
    </filter>
    <filter id="shadow" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur stdDeviation="20"/><feOffset dy="20"/>
      <feColorMatrix values="0 0 0 0 ${sr}  0 0 0 0 ${sg}  0 0 0 0 ${sb}  0 0 0 .38 0"/>
    </filter>
    <clipPath id="tile"><rect x="100" y="100" width="824" height="824" rx="186"/></clipPath>`;

  const scaled = (inner: string) =>
    fit === 1 ? inner : `<g transform="translate(512 512) scale(${fit.toFixed(4)}) translate(-512 -512)">${inner}</g>`;
  const tile = (x0: number, size: number) =>
    `<rect x="${x0}" y="${x0}" width="${size}" height="${size}" fill="url(#sky)"/><g mask="url(#vignette)">${grid(d)}</g>`;
  // Frost, then Edge light, over a soft tinted shadow.
  const mark = scaled(
    `<use href="#mark" fill="#000000" filter="url(#shadow)"/>` +
      `<use href="#mark" fill="url(#frost)" fill-opacity=".94"/>` +
      `<use href="#mark" fill="#FFFFFF" filter="url(#rim)"/>`,
  );

  const svg = (viewBox: string, body: string) =>
    `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="${viewBox}">\n` +
    `  <!-- ${d.name}'s App icon, generated by the solar-enterprise-app-icon kit from icon.json. Tile ${css(p.top)} -> ${css(p.bottom)}.${space === "p3" ? " Colours are Display P3 values: only correct in a file tagged with the P3 profile." : ""} -->\n` +
    `  <defs>${defs}\n  </defs>\n  ${body}\n</svg>\n`;

  // An Android layer is 108 dp with the middle 72 dp showing, so the 1024 artboard is its middle two thirds.
  const LAYER = "-256 -256 1536 1536";
  switch (part) {
    case "full":
      return svg("0 0 1024 1024", tile(0, 1024) + mark);
    case "desktop":
      // macOS and Windows icons: the Tile rounded, with a transparent margin around it.
      return svg("0 0 1024 1024", `<g clip-path="url(#tile)"><g transform="translate(100 100) scale(.8047)">${tile(-256, 1536)}${mark}</g></g>`);
    case "background":
      return svg(LAYER, tile(-256, 1536));
    case "foreground":
      return svg(LAYER, mark);
    case "monochrome":
      return svg(LAYER, scaled(`<use href="#mark" fill="#FFFFFF"/>`));
    case "mark":
      return svg("0 0 1024 1024", `<use href="#mark" fill="#000000"/>`);
  }
}
