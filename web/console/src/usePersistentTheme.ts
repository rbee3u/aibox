import { useLayoutEffect, useState } from "react";
import { readPreference, storePreference } from "./preferences";

const STORAGE_KEY = "aibox-console-theme";

export type ThemePreference = "system" | "light" | "dark";

export function usePersistentTheme(): readonly [ThemePreference, (theme: ThemePreference) => void] {
  const [theme, setTheme] = useState<ThemePreference>(readThemePreference);

  useLayoutEffect(() => {
    const root = document.documentElement;
    if (theme === "system") root.removeAttribute("data-theme");
    else root.dataset.theme = theme;
    storePreference(STORAGE_KEY, theme);
    return () => root.removeAttribute("data-theme");
  }, [theme]);

  return [theme, setTheme];
}

function readThemePreference(): ThemePreference {
  const value = readPreference(STORAGE_KEY);
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}
