// The app's UI accent on the icon's hue: a light-theme and a dark-theme value, found by moving only lightness
// until the accent passes as text on the app's surface (WCAG 4.5:1 and APCA |Lc| 60), plus the text colour that
// reads on the accent itself.
//   bun kit/accent.ts <hue> [light surface, default #FFFFFF] [dark surface, default #121214]
import { apca, ceilingP3, css, toHex, vivid, wcag } from "./oklch";

const [hueArg, lightSurface = "#FFFFFF", darkSurface = "#121214"] = process.argv.slice(2);
if (!hueArg) {
  console.error("usage: bun kit/accent.ts <hue> [light surface #hex] [dark surface #hex]");
  process.exit(1);
}
const hue = +hueArg;
const passes = (accent: string, surface: string) => wcag(accent, surface) >= 4.5 && Math.abs(apca(accent, surface)) >= 60;

function find(surface: string, from: number, to: number) {
  const step = from > to ? -0.005 : 0.005;
  for (let l = from; step < 0 ? l >= to : l <= to; l += step) {
    const o = vivid(l, hue, 0.9);
    const hex = toHex(o);
    if (passes(hex, surface)) {
      const onAccent = wcag("#FFFFFF", hex) >= wcag("#111114", hex) ? "#FFFFFF" : "#111114";
      return { o, hex, onAccent };
    }
  }
  throw new Error(`no accent on hue ${hue} passes on ${surface}`);
}

// Light theme: as light as it can be while still passing; dark theme: as dark as it can be while still passing.
for (const [theme, surface, from, to] of [
  ["light", lightSurface, 0.72, 0.25],
  ["dark", darkSurface, 0.5, 0.95],
] as const) {
  const a = find(surface, from, to);
  // The wide-gamut value: same lightness and hue, 90% of the P3 ceiling, for the @media (color-gamut: p3) override.
  const p3 = { ...a.o, c: ceilingP3(a.o.l, hue) * 0.9 };
  console.log(
    `${theme.padEnd(5)} accent ${css(a.o)} ${a.hex}  on ${surface}: WCAG ${wcag(a.hex, surface).toFixed(2)}, ` +
      `APCA ${apca(a.hex, surface).toFixed(0)}  · text on it: ${a.onAccent} (WCAG ${wcag(a.onAccent, a.hex).toFixed(2)})`,
  );
  console.log(`      P3    ${css(p3)}  (behind @media (color-gamut: p3); contrast is checked on the sRGB value above)`);
}
