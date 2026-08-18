import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { ControlApi } from "./controlApi";
import type { OverviewData } from "./controlApi";
import { recordList } from "./test/fixtures";

const overview = {
  version: "1.2.3",
  listen: "127.0.0.1:8080",
  uptime_seconds: 60,
  aibox_root: "/tmp/aibox",
  docker: "available",
  docker_error: null,
  runtime_image: "aibox:latest",
  image_available: true,
  managed_tenants: 1,
  request_records: 2,
  request_bytes: 1024,
  operation: null,
} satisfies OverviewData;

afterEach(() => {
  window.history.replaceState(null, "", "/");
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

describe("Console App", () => {
  it("renders the complete resource catalog in the sidebar", async () => {
    mockControlApi();
    render(<App />);

    await screen.findByRole("region", { name: "Service status" });
    const resources = screen.getByRole("navigation", { name: "Resources" });
    const expected = [
      ["GitHub", "https://github.com/rbee3u/aibox", "github"],
      ["Codex docs", "https://developers.openai.com/codex/cli", "codex"],
      ["Claude docs", "https://code.claude.com/docs/en/overview", "claude"],
    ];

    for (const [name, href, iconName] of expected) {
      const link = within(resources).getByRole("link", { name });
      expect(link).toHaveAttribute("href", href);
      expect(link).toHaveAttribute("target", "_blank");
      expect(link).toHaveAttribute("rel", "noopener noreferrer");
      const icon = link.querySelector<HTMLElement>("span");
      expect(icon?.style.getPropertyValue("--brand-icon")).toMatch(/^url\("data:image\/svg\+xml,/);
      expect(icon).toHaveAttribute("data-icon", iconName);
    }
    expect(within(screen.getByRole("banner")).queryByRole("link")).not.toBeInTheDocument();
    expect(screen.getByText("v1.2.3")).toBeInTheDocument();
  });

  it("uses domain-specific icons for the primary modules", async () => {
    mockControlApi();
    render(<App />);

    await screen.findByRole("region", { name: "Service status" });
    const modules = screen.getByRole("navigation", { name: "Modules" });
    const expected = [
      ["Overview", "overview", "lucide-layout-dashboard"],
      ["Tenants", "tenants", "lucide-users-round"],
      ["Configs", "configs", "lucide-file-sliders"],
      ["Sessions", "sessions", "lucide-messages-square"],
      ["Requests", "requests", "lucide-arrow-left-right"],
    ];

    for (const [label, iconName, iconClass] of expected) {
      const button = within(modules).getByRole("button", { name: new RegExp(`^${label}`) });
      expect(button.querySelector(`[data-icon="${iconName}"]`)).toHaveClass(iconClass);
    }
  });

  it("persists the desktop sidebar preference and defaults invalid values to expanded", async () => {
    window.localStorage.setItem("aibox-console-sidebar-collapsed", "invalid");
    mockControlApi();
    const user = userEvent.setup();
    const first = render(<App />);

    await screen.findByRole("region", { name: "Service status" });
    await user.click(screen.getByRole("button", { name: "Collapse sidebar" }));

    expect(screen.getByRole("button", { name: "Expand sidebar" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "GitHub" })).toHaveAttribute("title", "GitHub");
    await waitFor(() =>
      expect(window.localStorage.getItem("aibox-console-sidebar-collapsed")).toBe("true"),
    );

    first.unmount();
    render(<App />);
    await screen.findByRole("region", { name: "Service status" });

    expect(screen.getByRole("button", { name: "Expand sidebar" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand sidebar" }));
    expect(screen.getByRole("button", { name: "Collapse sidebar" })).toBeInTheDocument();
    await waitFor(() =>
      expect(window.localStorage.getItem("aibox-console-sidebar-collapsed")).toBe("false"),
    );
  });

  it("offers an accessible icon-backed theme menu", async () => {
    mockControlApi();
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("region", { name: "Service status" });
    const trigger = screen.getByRole("button", { name: "Color theme: System" });
    await user.click(trigger);

    const menu = screen.getByRole("menu", { name: "Color theme" });
    const system = within(menu).getByRole("menuitemradio", { name: "System" });
    expect(system).toHaveAttribute("aria-checked", "true");
    expect(system).toHaveFocus();
    expect(within(menu).getByRole("menuitemradio", { name: "Light" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(within(menu).getByRole("menuitemradio", { name: "Dark" })).toHaveAttribute(
      "aria-checked",
      "false",
    );

    await user.keyboard("{ArrowDown}{Enter}");

    expect(screen.queryByRole("menu", { name: "Color theme" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Color theme: Light" })).toHaveFocus();
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(window.localStorage.getItem("aibox-console-theme")).toBe("light");

    await user.click(screen.getByRole("button", { name: "Color theme: Light" }));
    await user.keyboard("{End}");
    expect(screen.getByRole("menuitemradio", { name: "Dark" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu", { name: "Color theme" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Color theme: Light" })).toHaveFocus();

    await user.click(screen.getByRole("button", { name: "Color theme: Light" }));
    await user.click(screen.getByRole("button", { name: /Overview/ }));
    expect(screen.queryByRole("menu", { name: "Color theme" })).not.toBeInTheDocument();
  });

  it("keeps the selected light theme when navigating from Requests to Overview", async () => {
    window.history.replaceState(null, "", "/_aibox/ui/requests");
    window.localStorage.setItem("aibox-console-theme", "light");
    mockControlApi();
    mockRequestFetch();

    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("complementary", { name: "Request Record list" });
    expect(screen.getByRole("button", { name: "Color theme: Light" })).toBeInTheDocument();
    expect(document.documentElement).toHaveAttribute("data-theme", "light");

    await user.click(screen.getByRole("button", { name: /Overview/ }));

    await screen.findByRole("region", { name: "Service status" });
    expect(screen.getByRole("button", { name: "Color theme: Light" })).toBeInTheDocument();
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });
});

function mockControlApi() {
  const api = {
    bootstrap: { version: "1.2.3", csrf_token: "token" },
    get: vi.fn((path: string) => {
      if (path === "/_aibox/api/operations/current") return Promise.resolve({ operation: null });
      if (path === "/_aibox/api/overview") return Promise.resolve(overview);
      return Promise.reject(new Error(`Unexpected Control API request: ${path}`));
    }),
  } as unknown as ControlApi;
  vi.spyOn(ControlApi, "connect").mockResolvedValue(api);
  vi.stubGlobal(
    "EventSource",
    class {
      addEventListener() {}
      close() {}
    },
  );
}

function mockRequestFetch() {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(
      new Response(JSON.stringify(recordList), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    ),
  );
}
