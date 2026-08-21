import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConsoleProvider } from "./ConsoleProvider";
import { usePersistentTheme } from "./usePersistentTheme";

function ThemeHarness() {
  usePersistentTheme();
  return <span>Console</span>;
}

describe("ConsoleProvider", () => {
  afterEach(() => {
    document.head.querySelector('meta[name="aibox-csp-nonce"]')?.remove();
    window.localStorage.clear();
  });

  it("passes the embedded CSP nonce to Ant Design generated styles", async () => {
    const meta = document.createElement("meta");
    meta.name = "aibox-csp-nonce";
    meta.content = "test-nonce";
    document.head.append(meta);

    render(
      <ConsoleProvider>
        <button type="button">Ready</button>
      </ConsoleProvider>,
    );

    expect(screen.getByRole("button", { name: "Ready" })).toBeInTheDocument();
    await waitFor(() => {
      expect(
        [...document.head.querySelectorAll("style")].some(
          (style) => style.getAttribute("nonce") === "test-nonce",
        ),
      ).toBe(true);
    });
  });

  it("keeps the Ant Design provider in the full-height Console layout chain", () => {
    render(
      <ConsoleProvider>
        <span>Console</span>
      </ConsoleProvider>,
    );

    expect(screen.getByText("Console").parentElement).toHaveClass("aibox-console-provider");
  });

  it("updates resolved theme tokens when the system preference changes", async () => {
    const systemChanges: Array<() => void> = [];
    const media = {
      matches: false,
      addEventListener: vi.fn((_event: string, listener: () => void) => {
        systemChanges.push(listener);
      }),
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue(media));

    render(
      <ConsoleProvider>
        <ThemeHarness />
      </ConsoleProvider>,
    );
    expect(document.documentElement).toHaveAttribute("data-resolved-theme", "light");

    media.matches = true;
    for (const listener of systemChanges) listener();
    await waitFor(() => {
      expect(document.documentElement).toHaveAttribute("data-resolved-theme", "dark");
    });
  });
});
