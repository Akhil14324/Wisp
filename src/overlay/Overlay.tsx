import { useRef, useState } from "react";
import { useEvent } from "../shared/events";
import { ipc } from "../shared/ipc";
import type { Stroke } from "../shared/types";
import { toPhysical } from "./geometry";

interface Init {
  monitor: string;
  width: number;
  height: number;
  scale: number;
  mode: "draw" | "rings";
}

interface Ring {
  id: number;
  x: number;
  y: number;
}

/** Full-screen overlay (section 3.3): Circle to Ask drawing, or Computer Use rings. */
export function Overlay() {
  const [mode, setMode] = useState<"draw" | "rings" | null>(null);
  const [strokes, setStrokes] = useState<[number, number][][]>([]);
  const [rings, setRings] = useState<Ring[]>([]);
  const drawing = useRef(false);
  const strokesRef = useRef<[number, number][][]>([]);
  const ringId = useRef(0);

  useEvent<Init>("overlay-init", (p) => {
    setMode(p.mode);
    strokesRef.current = [];
    setStrokes([]);
    setRings([]);
  });

  // Chord released: send strokes in physical, monitor-local pixels (FR-CTA-08).
  useEvent("overlay-finish", () => {
    const dpr = window.devicePixelRatio || 1;
    const out: Stroke[] = strokesRef.current.filter((s) => s.length > 0).map((s) => ({ points: toPhysical(s, dpr) }));
    ipc.overlaySubmit(out).catch(() => {});
    strokesRef.current = [];
    setStrokes([]);
    setMode(null);
  });

  useEvent<{ x: number; y: number }>("overlay-ring", (p) => {
    const dpr = window.devicePixelRatio || 1;
    const id = ++ringId.current;
    setRings((r) => [...r, { id, x: p.x / dpr, y: p.y / dpr }]);
    window.setTimeout(() => setRings((r) => r.filter((x) => x.id !== id)), 400);
  });

  const onDown = (e: React.PointerEvent) => {
    if (mode !== "draw" || e.button !== 0) return;
    (e.target as Element).setPointerCapture(e.pointerId);
    drawing.current = true;
    strokesRef.current = [...strokesRef.current, [[e.clientX, e.clientY]]];
    setStrokes(strokesRef.current);
  };
  const onMove = (e: React.PointerEvent) => {
    if (!drawing.current) return;
    const all = strokesRef.current;
    const last = all[all.length - 1];
    const prev = last[last.length - 1];
    if (Math.hypot(e.clientX - prev[0], e.clientY - prev[1]) < 2) return;
    last.push([e.clientX, e.clientY]);
    strokesRef.current = [...all];
    setStrokes(strokesRef.current);
  };
  const onUp = () => {
    drawing.current = false;
  };

  if (mode === "rings" || (mode === null && rings.length > 0)) {
    return (
      <div className="fixed inset-0 pointer-events-none">
        {rings.map((r) => (
          <span
            key={r.id}
            className="absolute rounded-full"
            style={{
              left: r.x,
              top: r.y,
              width: 44,
              height: 44,
              border: "3px solid var(--accent)",
              boxShadow: "0 0 18px var(--accent)",
              animation: "wisp-ring 400ms ease-out forwards",
            }}
          />
        ))}
      </div>
    );
  }
  if (mode !== "draw") return null;

  return (
    <div
      className="fixed inset-0"
      style={{ background: "rgba(0,0,0,0.2)", cursor: "crosshair" }}
      onPointerDown={onDown}
      onPointerMove={onMove}
      onPointerUp={onUp}
      onPointerCancel={onUp}
    >
      <svg className="absolute inset-0 w-full h-full pointer-events-none" aria-hidden>
        <defs>
          <filter id="glow" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="4" result="b" />
            <feMerge>
              <feMergeNode in="b" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>
        {strokes.map((s, i) => (
          <polyline
            key={i}
            points={s.map((p) => p.join(",")).join(" ")}
            fill="none"
            stroke="#ff4d4d"
            strokeWidth={4}
            strokeLinecap="round"
            strokeLinejoin="round"
            filter="url(#glow)"
          />
        ))}
      </svg>
      <div className="absolute top-4 left-1/2 -translate-x-1/2 rounded-full px-4 py-1.5 text-[13px] text-white" style={{ background: "rgba(16,16,20,0.82)" }}>
        Circle what you mean and ask. Release to send.
      </div>
    </div>
  );
}
