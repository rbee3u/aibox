import { useEffect, useLayoutEffect, useState } from "react";
import { readPreference, storePreference } from "./preferences";
import { consoleThemeTokens, resolveTheme } from "./themeTokens";

const STORAGE_KEY = "aibox-console-theme";
export const THEME_CHANGE_EVENT = "aibox-theme-change";

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
  const tokens = consoleThemeTokens[resolved];
  if (theme === "system") root.removeAttribute("data-theme");
  else root.dataset.theme = theme;
  root.dataset.resolvedTheme = resolved;
  root.style.setProperty("--aibox-canvas", tokens.canvas);
  root.style.setProperty("--aibox-shell", tokens.shell);
  root.style.setProperty("--aibox-surface", tokens.surface);
  root.style.setProperty("--aibox-surface-raised", tokens.raised);
  root.style.setProperty("--aibox-surface-inset", tokens.inset);
  root.style.setProperty("--aibox-surface-hover", tokens.hover);
  root.style.setProperty("--aibox-surface-selected", tokens.selected);
  root.style.setProperty("--aibox-line", tokens.line);
  root.style.setProperty("--aibox-line-soft", tokens.lineSoft);
  root.style.setProperty("--aibox-line-strong", tokens.lineStrong);
  root.style.setProperty("--aibox-text", tokens.ink);
  root.style.setProperty("--aibox-text-secondary", tokens.inkSecondary);
  root.style.setProperty("--aibox-text-muted", tokens.muted);
  root.style.setProperty("--aibox-text-faint", tokens.faint);
  root.style.setProperty("--aibox-accent", tokens.accent);
  root.style.setProperty("--aibox-accent-strong", tokens.accentStrong);
  root.style.setProperty("--aibox-accent-soft", tokens.accentSoft);
  root.style.setProperty("--aibox-accent-subtle", tokens.accentSubtle);
  root.style.setProperty("--aibox-focus", tokens.focus);
  root.style.setProperty("--aibox-danger", tokens.danger);
  root.style.setProperty("--aibox-danger-strong", tokens.dangerStrong);
  root.style.setProperty("--aibox-danger-soft", tokens.dangerSoft);
  root.style.setProperty("--aibox-danger-line", tokens.dangerLine);
  root.style.setProperty("--aibox-success", tokens.success);
  root.style.setProperty("--aibox-success-soft", tokens.successSoft);
  root.style.setProperty("--aibox-warning", tokens.warning);
  root.style.setProperty("--aibox-warning-soft", tokens.warningSoft);
  root.style.setProperty("--aibox-warning-line", tokens.warningLine);
  root.style.setProperty("--aibox-info-line", tokens.infoLine);
  root.style.setProperty("--aibox-code-bg", tokens.codeBackground);
  root.style.setProperty("--aibox-code-border", tokens.codeBorder);
  root.style.setProperty("--aibox-code-text", tokens.codeText);
  root.style.setProperty("--aibox-shadow-sm", tokens.shadowSmall);
  root.style.setProperty("--aibox-shadow-md", tokens.shadowMedium);
  window.dispatchEvent(new CustomEvent(THEME_CHANGE_EVENT, { detail: { theme, resolved } }));
}

export function readThemePreference(): ThemePreference {
  const value = readPreference(STORAGE_KEY);
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}
