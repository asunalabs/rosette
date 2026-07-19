"use client";

import { buildRosette } from "@/lib/rosette";
import { useTheme } from "@/components/theme-provider";

/**
 * The one visual signature (DESIGN.md): a deterministic guilloché identicon
 * derived from a contact's key fingerprint. Anti-forgery engraving heritage —
 * hard to forge, easy to recognize. Never a shield/warning/humanoid form.
 */
export function Rosette({
  fingerprint,
  size = 48,
  verified = false,
  className,
}: {
  fingerprint: string;
  size?: number;
  verified?: boolean;
  className?: string;
}) {
  const theme = useTheme();
  const { bgToken, bands, core, ring } = buildRosette(fingerprint, size, verified, theme);

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      className={className}
      role="img"
      aria-label={verified ? "Verified contact identicon" : "Contact identicon"}
    >
      <circle cx={50} cy={50} r={49} fill={`var(--${bgToken})`} stroke="var(--hairline)" strokeWidth={1} />
      {bands.map((band, i) => (
        <path key={i} d={band.d} fill="none" stroke={band.stroke} strokeWidth={band.strokeWidth} opacity={band.opacity} />
      ))}
      <circle cx={50} cy={50} r={core.r} fill={core.fill} />
      {ring && <path d={ring.d} fill="none" stroke={ring.stroke} strokeWidth={ring.strokeWidth} opacity={ring.opacity} />}
    </svg>
  );
}
