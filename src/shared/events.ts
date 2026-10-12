// Rust-to-UI events use the form wisp://<topic> (section 3.6).
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";

export type Topic =
  | "island-state"
  | "audio-level"
  | "confirm-request"
  | "confirm-closed"
  | "agent-step"
  | "agent-paused"
  | "overlay-init"
  | "overlay-finish"
  | "overlay-ring"
  | "card-update"
  | "settings-changed"
  | "presets-changed"
  | "todos-changed"
  | "jobs-changed"
  | "history-changed"
  | "media-changed"
  | "clipboard-changed"
  | "pins-changed"
  | "app-choices"
  | "job-created"
  | "navigate"
  | "devices-changed"
  | "toast";

export function on<T>(topic: Topic, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(`wisp://${topic}`, (e) => handler(e.payload));
}

/** Subscribes for the component's lifetime; the latest handler is always used. */
export function useEvent<T>(topic: Topic, handler: (payload: T) => void) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    let un: UnlistenFn | undefined;
    let cancelled = false;
    on<T>(topic, (p) => ref.current(p)).then((u) => {
      if (cancelled) u();
      else un = u;
    });
    return () => {
      cancelled = true;
      un?.();
    };
  }, [topic]);
}

/** Re-runs `load` on mount and whenever any of the topics fire. */
export function useReload(topics: Topic[], load: () => void) {
  const ref = useRef(load);
  ref.current = load;
  const key = topics.join(",");
  useEffect(() => {
    ref.current();
    const uns: Promise<UnlistenFn>[] = key.split(",").map((t) => on(t as Topic, () => ref.current()));
    return () => {
      uns.forEach((p) => p.then((u) => u()));
    };
  }, [key]);
}
