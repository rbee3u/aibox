import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyThemePreference,
  initializeThemePreference,
  usePersistentTheme,
} from "@/app/theme/usePersistentTheme";

function ThemeHarness() {
  const [theme, setTheme] = usePersistentTheme();
  return (
    <button type="button" onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>
      {theme}
    </button>
  );
}

describe("persistent Console theme", () => {
  afterEach(() => window.localStorage.clear());

  it("applies explicit themes through attributes without inline token values", () => {
    applyThemePreference("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(document.documentElement).toHaveAttribute("data-resolved-theme", "dark");
    expect(document.documentElement).not.toHaveAttribute("style");

    applyThemePreference("light");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(document.documentElement).toHaveAttribute("data-resolved-theme", "light");
  });

  it("initializes and follows the system preference", () => {
    const listeners: Array<() => void> = [];
    const media = {
      matches: false,
      addEventListener: vi.fn((_event: string, listener: () => void) => listeners.push(listener)),
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue(media));

    initializeThemePreference();
    expect(document.documentElement).not.toHaveAttribute("data-theme");
    expect(document.documentElement).toHaveAttribute("data-resolved-theme", "light");

    render(<ThemeHarness />);
    expect(screen.getByRole("button", { name: "system" })).toBeInTheDocument();
    media.matches = true;
    act(() => listeners.forEach((listener) => listener()));
    expect(document.documentElement).toHaveAttribute("data-resolved-theme", "dark");
  });

  it("persists explicit user choices", async () => {
    window.localStorage.setItem("aibox-console-theme", "dark");
    const user = userEvent.setup();
    render(<ThemeHarness />);

    expect(screen.getByRole("button", { name: "dark" })).toBeInTheDocument();
    expect(document.documentElement).toHaveAttribute("data-resolved-theme", "dark");
    await user.click(screen.getByRole("button", { name: "dark" }));
    expect(window.localStorage.getItem("aibox-console-theme")).toBe("light");
    expect(document.documentElement).toHaveAttribute("data-resolved-theme", "light");
  });
});
