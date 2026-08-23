import { useEffect, useLayoutEffect, useState } from "react";
import { readPreference, storePreference } from "./preferences";

const STORAGE_KEY = "aibox-console-theme";

export type ThemePreference = "system" | "light" | "dark";

export function usePersistentTheme(): readonly [ThemePreference, (theme: ThemePreference) => void] {
  const [theme, setTheme] = useState<ThemePreference>(readThemePreference);

  useLayoutEffect(() => {
    applyThemePreference(theme);
    storePreference(STORAGE_KEY, theme);
  }, [theme]);

  useEffect(() => {
    if (theme !== "system") return;
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    const update = () => applyThemePreference("system");
    media?.addEventListener?.("change", update);
    return () => media?.removeEventListener?.("change", update);
  }, [theme]);

  return [theme, setTheme];
}

export function initializeThemePreference(): void {
  applyThemePreference(readThemePreference());
}

export function applyThemePreference(theme: ThemePreference): void {
  const root = document.documentElement;
  const resolved = resolveTheme(theme);
  if (theme === "system") root.removeAttribute("data-theme");
  else root.dataset.theme = theme;
  root.dataset.resolvedTheme = resolved;
}

export function readThemePreference(): ThemePreference {
  const value = readPreference(STORAGE_KEY);
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}

function resolveTheme(preference: ThemePreference): "light" | "dark" {
  if (preference !== "system") return preference;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}
