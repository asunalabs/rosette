// Guilloché rosette identicon — ported verbatim from
// docs/design/design-preview.html (functions bytesFrom/bandPath/rosette).
// Deterministic per fingerprint; do not change the math, only the rendering
// (SVG string -> typed path data) when adapting to a new surface.

export type RosetteTheme = "light" | "dark";

const INKS: [string, string][] = [
  ["#5B3A6C", "#B08CC9"],
  ["#432852", "#9A77B5"],
  ["#6E4555", "#B07E92"],
  ["#7E2A3E", "#C98A97"],
  ["#5B4A68", "#9C89B0"],
  ["#4A5568", "#8B9BB4"],
  ["#2F4858", "#7593A6"],
  ["#365F71", "#6FA3B8"],
  ["#584338", "#A08575"],
  ["#706030", "#B5A253"],
  ["#3C3C3C", "#9A9A9A"],
  ["#432E3B", "#8E7185"],
];

function bytesFrom(hex: string): number[] {
  const bytes: number[] = [];
  for (let i = 0; i < hex.length - 1; i += 2) {
    bytes.push(parseInt(hex.slice(i, i + 2), 16));
  }
  return bytes;
}

function bandPath(
  cx: number,
  cy: number,
  base: number,
  amp1: number,
  amp2: number,
  k: number,
  ph: number,
  steps: number,
): string {
  let d = "";
  for (let i = 0; i <= steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const r = base + amp1 * Math.cos(k * t + ph) + amp2 * Math.cos(2 * k * t);
    d += (i === 0 ? "M" : "L") + (cx + r * Math.cos(t)).toFixed(2) + " " + (cy + r * Math.sin(t)).toFixed(2);
  }
  return d + "Z";
}

export interface RosetteBand {
  d: string;
  stroke: string;
  strokeWidth: number;
  opacity: number;
}

export interface RosetteData {
  bgToken: "surface" | "surface-2";
  bands: RosetteBand[];
  core: { r: number; fill: string };
  ring: RosetteBand | null;
}

/** size >= 56 gets 5 bands @ thinner stroke; below that, 3 bands @ thicker stroke (stays legible small). */
export function buildRosette(fingerprintHex: string, size: number, verified: boolean, theme: RosetteTheme): RosetteData {
  const B = bytesFrom(fingerprintHex);
  const dark = theme === "dark";
  const shade = dark ? 1 : 0;
  const ink = INKS[B[0] % 12][shade];
  const ink2 = INKS[((B[0] % 12) + 3 + (B[1] % 5)) % 12][shade];

  const k = 5 + (B[2] % 5);
  const base = 26 + (B[3] % 8);
  const amp1 = 6 + (B[4] % 9);
  const amp2 = 1 + (B[5] % 5);
  const ph0 = ((B[6] % 64) / 64) * Math.PI * 2;
  const n = size >= 56 ? 5 : 3;
  const strokeWidth = size >= 56 ? 1.1 : 1.7;

  const bands: RosetteBand[] = [];
  for (let i = 0; i < n; i++) {
    const shrink = 1 - i * (0.1 + (B[7] % 4) * 0.012);
    const ph = ph0 + i * ((B[8] % 16) / 40);
    bands.push({
      d: bandPath(50, 50, base * shrink, amp1 * shrink, amp2, k, ph, 220),
      stroke: i === n - 2 ? ink2 : ink,
      strokeWidth,
      opacity: i === 0 ? 0.95 : 0.75,
    });
  }

  const core = { r: 2.4 + (B[9] % 4), fill: ink };

  const ring: RosetteBand | null = verified
    ? {
        d: bandPath(50, 50, 45.5, 1.6, 0.6, k * 3, ph0, 260),
        stroke: ink,
        strokeWidth: size >= 56 ? 0.9 : 1.3,
        opacity: 0.95,
      }
    : null;

  return { bgToken: dark ? "surface-2" : "surface", bands, core, ring };
}
