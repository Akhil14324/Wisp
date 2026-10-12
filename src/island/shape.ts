// Island shapes. Width is fixed per kind; height follows the content.

export type ShapeKind = "pill" | "compact" | "wide" | "input" | "panel" | "card";

export interface Shape {
  width: number;
  /** Largest corner radius; small shapes become a full pill (height / 2). */
  maxRadius: number;
}

const SHAPES: Record<ShapeKind, Shape> = {
  pill: { width: 140, maxRadius: 17 },
  compact: { width: 168, maxRadius: 17 },
  wide: { width: 420, maxRadius: 20 },
  input: { width: 420, maxRadius: 22 },
  panel: { width: 420, maxRadius: 22 },
  card: { width: 420, maxRadius: 20 },
};

/** The Computer Use banner needs the wide shape whatever the state. */
export function shapeFor(kind: ShapeKind, controlling: boolean): Shape {
  if (controlling && (kind === "pill" || kind === "compact")) return SHAPES.wide;
  return SHAPES[kind];
}
