// oklch <-> sRGB, gamut, and contrast (WCAG 2 and APCA), for choosing and checking an icon's colours.

export type Oklch = { l: number; c: number; h: number };

const lin = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const gam = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

/** Linear sRGB channels of an oklch colour; outside 0..1 means it is out of the sRGB gamut. */
export function toLinear({ l, c, h }: Oklch): [number, number, number] {
  const a = c * Math.cos((h * Math.PI) / 180);
  const b = c * Math.sin((h * Math.PI) / 180);
  const L = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const M = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const S = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * L - 3.3077115913 * M + 0.2309699292 * S,
    -1.2684380046 * L + 2.6097574011 * M - 0.3413193965 * S,
    -0.0041960863 * L - 0.7034186147 * M + 1.707614701 * S,
  ];
}

export function fromHex(hex: string): Oklch {
  const [r, g, b] = [1, 3, 5].map((i) => lin(parseInt(hex.slice(i, i + 2), 16) / 255));
  const L = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const M = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const S = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const l = 0.2104542553 * L + 0.793617785 * M - 0.0040720468 * S;
  const a = 1.9779984951 * L - 2.428592205 * M + 0.4505937099 * S;
  const bb = 0.0259040371 * L + 0.7827717662 * M - 0.808675766 * S;
  return { l, c: Math.hypot(a, bb), h: (((Math.atan2(bb, a) * 180) / Math.PI) % 360 + 360) % 360 };
}

export const inGamut = (o: Oklch) => toLinear(o).every((v) => v >= -1e-4 && v <= 1 + 1e-4);

/** Linear Display P3 channels of an oklch colour (linear sRGB -> XYZ D65 -> linear P3). */
export function toLinearP3(o: Oklch): [number, number, number] {
  const [r, g, b] = toLinear(o);
  const x = 0.4123908 * r + 0.3575843 * g + 0.1804808 * b;
  const y = 0.212639 * r + 0.7151687 * g + 0.0721923 * b;
  const z = 0.0193308 * r + 0.1191948 * g + 0.9505322 * b;
  return [
    2.4934969 * x - 0.9313836 * y - 0.4027108 * z,
    -0.829489 * x + 1.7626641 * y + 0.0236247 * z,
    0.0358458 * x - 0.0761724 * y + 0.9568845 * z,
  ];
}

/** Whether a Display P3 screen can show the colour. */
export const inP3 = (o: Oklch) => toLinearP3(o).every((v) => v >= -1e-4 && v <= 1 + 1e-4);

/** A colour as Display P3 pixel values (P3 uses sRGB's transfer curve), its chroma clamped to the P3 gamut. Only
 *  meaningful in a file tagged as Display P3 (see icc.ts). */
export function toHexP3(o: Oklch): string {
  const fit = inP3(o) ? o : { ...o, c: Math.min(o.c, ceilingP3(o.l, o.h)) };
  return (
    "#" +
    toLinearP3(fit)
      .map((v) => Math.round(Math.min(1, Math.max(0, gam(v))) * 255).toString(16).padStart(2, "0"))
      .join("")
      .toUpperCase()
  );
}

function highest(fits: (o: Oklch) => boolean, l: number, h: number): number {
  let lo = 0;
  let hi = 0.5;
  for (let i = 0; i < 40; i++) {
    const mid = (lo + hi) / 2;
    if (fits({ l, c: mid, h })) lo = mid;
    else hi = mid;
  }
  return lo;
}

/** The most chroma an sRGB screen can show at this lightness and hue. */
export const ceiling = (l: number, h: number) => highest(inGamut, l, h);

/** The most chroma a Display P3 screen can show at this lightness and hue. */
export const ceilingP3 = (l: number, h: number) => highest(inP3, l, h);

/** A colour at `fraction` of its hue's chroma ceiling: siblings at one fraction look equally vivid. */
export const vivid = (l: number, h: number, fraction: number): Oklch => ({ l, c: ceiling(l, h) * fraction, h });

/** An oklch colour from linear sRGB channels. */
export function fromLinear([r, g, b]: [number, number, number]): Oklch {
  const [L, M, S] = [
    0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b,
    0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b,
    0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b,
  ].map(Math.cbrt);
  const l = 0.2104542553 * L + 0.793617785 * M - 0.0040720468 * S;
  const a = 1.9779984951 * L - 2.428592205 * M + 0.4505937099 * S;
  const bb = 0.0259040371 * L + 0.7827717662 * M - 0.808675766 * S;
  return { l, c: Math.hypot(a, bb), h: (((Math.atan2(bb, a) * 180) / Math.PI) % 360 + 360) % 360 };
}

/** Perceptual distance (in oklab). About 0.02 is just noticeable. */
export function deltaE(x: Oklch, y: Oklch): number {
  const ab = (o: Oklch) => [o.c * Math.cos((o.h * Math.PI) / 180), o.c * Math.sin((o.h * Math.PI) / 180)];
  const [a1, b1] = ab(x);
  const [a2, b2] = ab(y);
  return Math.hypot(x.l - y.l, a1 - a2, b1 - b2);
}

/** A wide-gamut version of an sRGB colour for a file tagged Display P3: the same lightness and hue with as much
 *  more chroma as P3 allows (the same fraction of the P3 ceiling as of the sRGB one), but only so far that an sRGB
 *  screen, which clips it, still shows it within `budget` (just noticeable) of the original. */
export function widen(o: Oklch, budget = 0.02): Oklch {
  const target = (o.c / ceiling(o.l, o.h)) * ceilingP3(o.l, o.h);
  if (target <= o.c) return o;
  const onSrgb = (c: number) => fromLinear(toLinear({ ...o, c }).map((v) => Math.min(1, Math.max(0, v))) as [number, number, number]);
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 30; i++) {
    const mid = (lo + hi) / 2;
    if (deltaE(o, onSrgb(o.c + (target - o.c) * mid)) <= budget) lo = mid;
    else hi = mid;
  }
  return { ...o, c: o.c + (target - o.c) * lo };
}

/** Clamps chroma (keeping lightness and hue) until the colour fits sRGB. */
export function clamp(o: Oklch): Oklch {
  return inGamut(o) ? o : { ...o, c: Math.min(o.c, ceiling(o.l, o.h)) };
}

export function toHex(o: Oklch): string {
  return (
    "#" +
    toLinear(clamp(o))
      .map((v) => Math.round(Math.min(1, Math.max(0, gam(v))) * 255).toString(16).padStart(2, "0"))
      .join("")
      .toUpperCase()
  );
}

export const css = ({ l, c, h }: Oklch) => `oklch(${+l.toFixed(3)} ${+c.toFixed(3)} ${+h.toFixed(1)})`;

/** Reads "oklch(L C H)", "#RRGGBB", or an {l, c, h} object. */
export function parse(value: string | Oklch): Oklch {
  if (typeof value !== "string") return value;
  if (value.startsWith("#")) return fromHex(value);
  const m = value.match(/oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\)/);
  if (!m) throw new Error(`not an oklch() or #hex colour: ${value}`);
  return { l: +m[1], c: +m[2], h: +m[3] };
}

const luminance = (hex: string) => {
  const [r, g, b] = [1, 3, 5].map((i) => lin(parseInt(hex.slice(i, i + 2), 16) / 255));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

/** WCAG 2 contrast ratio. */
export function wcag(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** APCA lightness contrast (Lc) of text on a background; its sign says which is lighter. */
export function apca(text: string, background: string): number {
  const y = (hex: string) => {
    const [r, g, b] = [1, 3, 5].map((i) => (parseInt(hex.slice(i, i + 2), 16) / 255) ** 2.4);
    const v = 0.2126729 * r + 0.7151522 * g + 0.072175 * b;
    return v > 0.022 ? v : v + (0.022 - v) ** 1.414;
  };
  const t = y(text);
  const bg = y(background);
  if (Math.abs(bg - t) < 0.0005) return 0;
  if (bg > t) {
    const s = (bg ** 0.56 - t ** 0.57) * 1.14;
    return s < 0.1 ? 0 : (s - 0.027) * 100;
  }
  const s = (bg ** 0.65 - t ** 0.62) * 1.14;
  return s > -0.1 ? 0 : (s + 0.027) * 100;
}
