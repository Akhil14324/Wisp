// Skin slot (section 4.10, 8.3): applies the active Skin preset live.
import { useEffect, useState } from "react";
import { on } from "./events";
import { ipc } from "./ipc";
import type { Skin } from "./types";

export const DEFAULT_SKIN: Skin = { theme: "dark", accent: "#8b9cff", radius: 8, opacity: 0.82, font_size: 13 };

function prefersDark(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-color-scheme: dark)").matches;
}

export function resolvedTheme(skin: Skin): "light" | "dark" {
  if (skin.theme === "auto") return prefersDark() ? "dark" : "light";
  return skin.theme;
}

export function applySkin(skin: Skin, animations = true) {
  const root = document.documentElement;
  root.dataset.theme = resolvedTheme(skin);
  root.dataset.anim = animations ? "on" : "off";
  root.style.setProperty("--accent", skin.accent);
  root.style.setProperty("--radius", `${skin.radius}px`);
  root.style.setProperty("--island-opacity", String(skin.opacity));
  root.style.setProperty("--island-font", `${skin.font_size}px`);
}

async function loadSkin(): Promise<Skin> {
  try {
    const presets = await ipc.presetsList();
    const active = presets.find((p) => p.slot === "skin" && p.is_active);
    return (active?.data as Skin) ?? DEFAULT_SKIN;
  } catch {
    return DEFAULT_SKIN;
  }
}

/** Loads the skin, re-applies it when presets or the OS theme change. */
export function useSkin(): Skin {
  const [skin, setSkin] = useState<Skin>(DEFAULT_SKIN);
  useEffect(() => {
    let alive = true;
    let animations = !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    const refresh = async () => {
      const s = await loadSkin();
      try {
        animations = (await ipc.appStatus()).animations && animations;
      } catch {
        /* window may not be allowed to call app_status */
      }
      if (!alive) return;
      setSkin(s);
      applySkin(s, animations);
    };
    refresh();
    const un = on("presets-changed", refresh);
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onScheme = () => refresh();
    mq?.addEventListener("change", onScheme);
    return () => {
      alive = false;
      un.then((u) => u());
      mq?.removeEventListener("change", onScheme);
    };
  }, []);
  return skin;
}
