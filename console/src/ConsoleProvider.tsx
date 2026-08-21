import { App as AntApp, ConfigProvider, theme as antdTheme } from "antd";
import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { consoleThemeTokens, resolveTheme, type ResolvedTheme } from "./themeTokens";
import { THEME_CHANGE_EVENT } from "./usePersistentTheme";

export function ConsoleProvider({ children }: { children: ReactNode }) {
  const [resolvedTheme, setResolvedTheme] = useState<ResolvedTheme>(() => {
    const resolved = document.documentElement.dataset.resolvedTheme;
    if (resolved === "dark" || resolved === "light") return resolved;
    const explicit = document.documentElement.dataset.theme;
    return explicit === "dark" || explicit === "light" ? explicit : resolveTheme("system");
  });

  useEffect(() => {
    const root = document.documentElement;
    const update = () => {
      const explicit = root.dataset.theme;
      setResolvedTheme(
        explicit === "dark" || explicit === "light" ? explicit : resolveTheme("system"),
      );
    };
    update();
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    media?.addEventListener?.("change", update);
    window.addEventListener(THEME_CHANGE_EVENT, update);
    return () => {
      media?.removeEventListener?.("change", update);
      window.removeEventListener(THEME_CHANGE_EVENT, update);
    };
  }, []);

  const tokens = consoleThemeTokens[resolvedTheme];
  const theme = useMemo(
    () => ({
      algorithm: resolvedTheme === "dark" ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
      token: {
        colorPrimary: tokens.accent,
        colorInfo: tokens.accent,
        colorSuccess: tokens.success,
        colorWarning: tokens.warning,
        colorError: tokens.danger,
        colorText: tokens.ink,
        colorTextSecondary: tokens.inkSecondary,
        colorTextTertiary: tokens.muted,
        colorBgBase: tokens.canvas,
        colorBgContainer: tokens.raised,
        colorBgElevated: tokens.raised,
        colorBorder: tokens.line,
        colorBorderSecondary: tokens.lineSoft,
        borderRadius: 6,
        borderRadiusSM: 5,
        borderRadiusLG: 8,
        controlHeight: 32,
        controlHeightSM: 28,
        controlHeightLG: 36,
        fontFamily: "var(--font-sans)",
        fontFamilyCode: "var(--font-mono)",
        motionDurationFast: "140ms",
        motionDurationMid: "180ms",
        boxShadowSecondary: tokens.shadowMedium,
      },
      components: {
        Button: {
          controlHeight: 32,
          paddingInline: 10,
          fontWeight: 600,
          primaryShadow: "none",
          defaultShadow: "none",
        },
        Input: {
          activeShadow: `0 0 0 2px color-mix(in srgb, ${tokens.focus} 24%, transparent)`,
        },
        Select: { optionSelectedBg: tokens.selected, optionActiveBg: tokens.hover },
        Tabs: {
          inkBarColor: tokens.accent,
          itemSelectedColor: tokens.accent,
          itemHoverColor: tokens.accent,
        },
        Modal: { borderRadiusLG: 8, contentBg: tokens.raised },
        Dropdown: { paddingBlock: 4 },
        Tooltip: { colorBgSpotlight: tokens.ink, colorTextLightSolid: tokens.canvas },
        Alert: { withDescriptionPadding: "10px 12px" },
      },
    }),
    [resolvedTheme, tokens],
  );

  const nonce = document.querySelector<HTMLMetaElement>('meta[name="aibox-csp-nonce"]')?.content;

  return (
    <ConfigProvider
      csp={nonce ? { nonce } : undefined}
      prefixCls="aibox"
      theme={theme}
      componentSize="middle"
    >
      <AntApp
        className="aibox-console-provider"
        message={{ maxCount: 3 }}
        notification={{ maxCount: 3 }}
      >
        {children}
      </AntApp>
    </ConfigProvider>
  );
}
