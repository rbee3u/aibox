export type ResolvedTheme = "light" | "dark";

export interface ConsoleThemeTokens {
  canvas: string;
  shell: string;
  surface: string;
  raised: string;
  inset: string;
  hover: string;
  selected: string;
  line: string;
  lineSoft: string;
  lineStrong: string;
  ink: string;
  inkSecondary: string;
  muted: string;
  faint: string;
  accent: string;
  accentStrong: string;
  accentSoft: string;
  accentSubtle: string;
  focus: string;
  danger: string;
  dangerStrong: string;
  dangerSoft: string;
  dangerLine: string;
  success: string;
  successSoft: string;
  warning: string;
  warningSoft: string;
  warningLine: string;
  infoLine: string;
  codeBackground: string;
  codeBorder: string;
  codeText: string;
  shadowSmall: string;
  shadowMedium: string;
}

export const consoleThemeTokens: Record<ResolvedTheme, ConsoleThemeTokens> = {
  light: {
    canvas: "#eef1f5",
    shell: "#f7f8fa",
    surface: "#ffffff",
    raised: "#ffffff",
    inset: "#f4f6f9",
    hover: "#f4f3ff",
    selected: "#eceaff",
    line: "#dce1e8",
    lineSoft: "#e9edf2",
    lineStrong: "#c5ccd6",
    ink: "#18202c",
    inkSecondary: "#465264",
    muted: "#657187",
    faint: "#59667a",
    accent: "#5b55e7",
    accentStrong: "#4943cb",
    accentSoft: "#efeeff",
    accentSubtle: "#f8f7ff",
    focus: "#7771f3",
    danger: "#ba2f38",
    dangerStrong: "#96232b",
    dangerSoft: "#fff0f1",
    dangerLine: "#efc7ca",
    success: "#0d7a55",
    successSoft: "#e8f7f1",
    warning: "#92500d",
    warningSoft: "#fff6e8",
    warningLine: "#ead2a8",
    infoLine: "#d5d2ff",
    codeBackground: "#f6f7fa",
    codeBorder: "#d9dfe7",
    codeText: "#253042",
    shadowSmall: "0 1px 2px rgb(18 28 45 / 0.06)",
    shadowMedium: "0 14px 42px rgb(18 28 45 / 0.16)",
  },
  dark: {
    canvas: "#10131a",
    shell: "#141820",
    surface: "#191e28",
    raised: "#1d2330",
    inset: "#151a23",
    hover: "#23243a",
    selected: "#292845",
    line: "#303744",
    lineSoft: "#272e3a",
    lineStrong: "#465061",
    ink: "#e7ebf2",
    inkSecondary: "#bdc5d2",
    muted: "#9ba6b7",
    faint: "#aab4c3",
    accent: "#9b96ff",
    accentStrong: "#8580ee",
    accentSoft: "#292744",
    accentSubtle: "#211f35",
    focus: "#aaa6ff",
    danger: "#ff858c",
    dangerStrong: "#ffabb0",
    dangerSoft: "#3a2027",
    dangerLine: "#6a343e",
    success: "#5bd4a4",
    successSoft: "#19372f",
    warning: "#f1b262",
    warningSoft: "#382b1d",
    warningLine: "#655036",
    infoLine: "#4a4778",
    codeBackground: "#11151d",
    codeBorder: "#313947",
    codeText: "#d9e0eb",
    shadowSmall: "0 1px 2px rgb(0 0 0 / 0.25)",
    shadowMedium: "0 18px 50px rgb(0 0 0 / 0.42)",
  },
};

export function resolveTheme(preference: "system" | ResolvedTheme): ResolvedTheme {
  if (preference !== "system") return preference;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}
