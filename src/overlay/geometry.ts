// CSS pixels to physical pixels for the overlay (FR-CTA-08).
export function toPhysical(points: [number, number][], dpr: number): [number, number][] {
  return points.map(([x, y]) => [Math.round(x * dpr * 10) / 10, Math.round(y * dpr * 10) / 10]);
}
