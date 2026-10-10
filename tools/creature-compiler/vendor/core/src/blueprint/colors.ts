/** CSS named colours, accepted as friendly forms and normalized to hex. */
const NAMED: Record<string, string> = {
  aliceblue: 'f0f8ff',
  antiquewhite: 'faebd7',
  aqua: '00ffff',
  aquamarine: '7fffd4',
  azure: 'f0ffff',
  beige: 'f5f5dc',
  bisque: 'ffe4c4',
  black: '000000',
  blanchedalmond: 'ffebcd',
  blue: '0000ff',
  blueviolet: '8a2be2',
  brown: 'a52a2a',
  burlywood: 'deb887',
  cadetblue: '5f9ea0',
  chartreuse: '7fff00',
  chocolate: 'd2691e',
  coral: 'ff7f50',
  cornflowerblue: '6495ed',
  cornsilk: 'fff8dc',
  crimson: 'dc143c',
  cyan: '00ffff',
  darkblue: '00008b',
  darkcyan: '008b8b',
  darkgoldenrod: 'b8860b',
  darkgray: 'a9a9a9',
  darkgreen: '006400',
  darkgrey: 'a9a9a9',
  darkkhaki: 'bdb76b',
  darkmagenta: '8b008b',
  darkolivegreen: '556b2f',
  darkorange: 'ff8c00',
  darkorchid: '9932cc',
  darkred: '8b0000',
  darksalmon: 'e9967a',
  darkseagreen: '8fbc8f',
  darkslateblue: '483d8b',
  darkslategray: '2f4f4f',
  darkslategrey: '2f4f4f',
  darkturquoise: '00ced1',
  darkviolet: '9400d3',
  deeppink: 'ff1493',
  deepskyblue: '00bfff',
  dimgray: '696969',
  dimgrey: '696969',
  dodgerblue: '1e90ff',
  firebrick: 'b22222',
  floralwhite: 'fffaf0',
  forestgreen: '228b22',
  fuchsia: 'ff00ff',
  gainsboro: 'dcdcdc',
  ghostwhite: 'f8f8ff',
  gold: 'ffd700',
  goldenrod: 'daa520',
  gray: '808080',
  green: '008000',
  greenyellow: 'adff2f',
  grey: '808080',
  honeydew: 'f0fff0',
  hotpink: 'ff69b4',
  indianred: 'cd5c5c',
  indigo: '4b0082',
  ivory: 'fffff0',
  khaki: 'f0e68c',
  lavender: 'e6e6fa',
  lavenderblush: 'fff0f5',
  lawngreen: '7cfc00',
  lemonchiffon: 'fffacd',
  lightblue: 'add8e6',
  lightcoral: 'f08080',
  lightcyan: 'e0ffff',
  lightgoldenrodyellow: 'fafad2',
  lightgray: 'd3d3d3',
  lightgreen: '90ee90',
  lightgrey: 'd3d3d3',
  lightpink: 'ffb6c1',
  lightsalmon: 'ffa07a',
  lightseagreen: '20b2aa',
  lightskyblue: '87cefa',
  lightslategray: '778899',
  lightslategrey: '778899',
  lightsteelblue: 'b0c4de',
  lightyellow: 'ffffe0',
  lime: '00ff00',
  limegreen: '32cd32',
  linen: 'faf0e6',
  magenta: 'ff00ff',
  maroon: '800000',
  mediumaquamarine: '66cdaa',
  mediumblue: '0000cd',
  mediumorchid: 'ba55d3',
  mediumpurple: '9370db',
  mediumseagreen: '3cb371',
  mediumslateblue: '7b68ee',
  mediumspringgreen: '00fa9a',
  mediumturquoise: '48d1cc',
  mediumvioletred: 'c71585',
  midnightblue: '191970',
  mintcream: 'f5fffa',
  mistyrose: 'ffe4e1',
  moccasin: 'ffe4b5',
  navajowhite: 'ffdead',
  navy: '000080',
  oldlace: 'fdf5e6',
  olive: '808000',
  olivedrab: '6b8e23',
  orange: 'ffa500',
  orangered: 'ff4500',
  orchid: 'da70d6',
  palegoldenrod: 'eee8aa',
  palegreen: '98fb98',
  paleturquoise: 'afeeee',
  palevioletred: 'db7093',
  papayawhip: 'ffefd5',
  peachpuff: 'ffdab9',
  peru: 'cd853f',
  pink: 'ffc0cb',
  plum: 'dda0dd',
  powderblue: 'b0e0e6',
  purple: '800080',
  rebeccapurple: '663399',
  red: 'ff0000',
  rosybrown: 'bc8f8f',
  royalblue: '4169e1',
  saddlebrown: '8b4513',
  salmon: 'fa8072',
  sandybrown: 'f4a460',
  seagreen: '2e8b57',
  seashell: 'fff5ee',
  sienna: 'a0522d',
  silver: 'c0c0c0',
  skyblue: '87ceeb',
  slateblue: '6a5acd',
  slategray: '708090',
  slategrey: '708090',
  snow: 'fffafa',
  springgreen: '00ff7f',
  steelblue: '4682b4',
  tan: 'd2b48c',
  teal: '008080',
  thistle: 'd8bfd8',
  tomato: 'ff6347',
  turquoise: '40e0d0',
  violet: 'ee82ee',
  wheat: 'f5deb3',
  white: 'ffffff',
  whitesmoke: 'f5f5f5',
  yellow: 'ffff00',
  yellowgreen: '9acd32',
};

export const COLOR_NAMES: readonly string[] = Object.keys(NAMED);

const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;

/** True for `#rgb`, `#rrggbb` or a CSS colour name. */
export function isColor(value: string): boolean {
  return HEX.test(value) || value.toLowerCase() in NAMED;
}

/** `#rrggbb` (lowercase) for any accepted colour, or undefined. */
export function toHex(value: string): string | undefined {
  if (HEX.test(value)) {
    const v = value.slice(1).toLowerCase();
    return v.length === 3 ? `#${v[0]}${v[0]}${v[1]}${v[1]}${v[2]}${v[2]}` : `#${v}`;
  }
  const named = NAMED[value.toLowerCase()];
  return named === undefined ? undefined : `#${named}`;
}

/** Linear-ish RGB triple in 0..1 from `#rrggbb` (sRGB values, not linearized). */
export function hexToRgb(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.slice(1), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

/**
 * The plain name of a colour for descriptions ("dark green", "greyish blue", "sand"), chosen by
 * hue, saturation and lightness, so a dull olive is not called tan nor a slate grey teal.
 */
export function colorName(color: string): string {
  const hex = toHex(color);
  if (!hex) return color;
  const [r, g, b] = [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16) / 255) as [
    number,
    number,
    number,
  ];
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  if (d < 0.06 || s < 0.12)
    return l < 0.1
      ? 'black'
      : l < 0.3
        ? 'charcoal'
        : l < 0.6
          ? 'grey'
          : l < 0.85
            ? 'pale grey'
            : 'white';
  const h =
    max === r
      ? (60 * ((g - b) / d) + 360) % 360
      : max === g
        ? 60 * ((b - r) / d + 2)
        : 60 * ((r - g) / d + 4);
  const shade = (name: string) => (l < 0.22 ? `dark ${name}` : l > 0.75 ? `pale ${name}` : name);
  const muted = (name: string) => (s < 0.3 ? `greyish ${name}` : shade(name));
  // Orange and yellow hues turn brown when dark or dull, and beige when light and dull.
  if (h >= 15 && h < 50) {
    if (l < 0.45 && (s < 0.75 || l < 0.3)) return l < 0.2 ? 'dark brown' : 'brown';
    if (l > 0.6 && s < 0.6) return l > 0.82 ? 'cream' : h < 35 ? 'tan' : 'sand';
    return h < 38 ? shade('orange') : 'golden';
  }
  if (h < 15 || h >= 345) return l > 0.7 ? 'pink' : shade('red');
  if (h < 75) return l < 0.45 ? 'olive' : shade('yellow');
  if (h < 160) return muted('green');
  if (h < 200) return muted('teal');
  if (h < 255) return muted('blue');
  if (h < 290) return muted('purple');
  return l > 0.6 ? 'pink' : shade('magenta');
}

/** Ways `palette.harmony` relates the accent's hue to the base's. */
export const HARMONIES = ['analogous', 'complementary', 'triadic', 'split', 'monochrome'] as const;
export type Harmony = (typeof HARMONIES)[number];

/** `#rrggbb` from hue (degrees), saturation and lightness (0 to 1). */
export function hslToHex(hue: number, saturation: number, lightness: number): string {
  const h = ((hue % 360) + 360) % 360;
  const s = Math.min(1, Math.max(0, saturation));
  const l = Math.min(0.95, Math.max(0.04, lightness));
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))));
  };
  return `#${[f(0), f(8), f(4)].map((c) => c.toString(16).padStart(2, '0')).join('')}`;
}

/** Hue (degrees), saturation and lightness (0 to 1) of `#rrggbb`. */
export function hexToHsl(hex: string): [number, number, number] {
  const [r, g, b] = hexToRgb(hex);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return [0, 0, l];
  const s = d / (1 - Math.abs(2 * l - 1));
  const h =
    max === r
      ? (60 * ((g - b) / d) + 360) % 360
      : max === g
        ? 60 * ((b - r) / d + 2)
        : 60 * ((r - g) / d + 4);
  return [h, s, l];
}

/**
 * Base, belly and accent colours in a harmony, from `random` (0 to 1 draws), around `base`'s hue
 * when one is given. Value contrast keeps patterns readable: the belly is at least 0.18 lighter
 * than the base, and the accent at least 0.18 away from it.
 */
export function harmonyPalette(
  harmony: Harmony,
  random: () => number,
  base?: string,
): { base: string; belly: string; accent: string } {
  const between = (lo: number, hi: number) => lo + (hi - lo) * random();
  const [hue, sat, light] = base
    ? hexToHsl(base)
    : [between(0, 360), between(0.25, 0.6), between(0.22, 0.45)];
  const offset = {
    analogous: between(25, 45) * (random() < 0.5 ? -1 : 1),
    complementary: 180 + between(-12, 12),
    triadic: 120 * (random() < 0.5 ? -1 : 1),
    split: 180 + 30 * (random() < 0.5 ? -1 : 1),
    monochrome: 0,
  }[harmony];
  // The accent goes darker on light bases and lighter on dark ones, with room to spare.
  // The accent goes darker on light bases and lighter on dark ones, keeping enough lightness
  // for its hue to show.
  const accentLight =
    light >= 0.34 ? Math.max(0.14, light - between(0.18, 0.26)) : light + between(0.2, 0.3);
  const bellyLight = Math.min(0.9, Math.max(light + 0.18, light + between(0.22, 0.35)));
  return {
    base: base ?? hslToHex(hue, sat, light),
    belly: hslToHex(hue + between(-8, 18), sat * 0.55, bellyLight),
    accent: hslToHex(hue + offset, Math.min(1, sat * between(0.9, 1.3)), accentLight),
  };
}
