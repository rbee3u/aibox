import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { ControlApi } from "./controlApi";
import type { ConfigListData, OverviewData, TenantRow, TopologyData } from "./controlApi";
import { materializeControlApi } from "./managementTestSupport";
import { recordList } from "./test/fixtures";

const overview = {
  service: {
    version: "1.2.3",
    listen: "127.0.0.1:8080",
    uptime_seconds: 60,
    aibox_root: "/tmp/aibox",
  },
  docker: { status: "available", error: null },
  runtime_image: {
    reference: "aibox:latest",
    status: "built",
    id: "sha256:1234567890abcdef",
    created_at: "2026-08-18T12:00:00Z",
    size_bytes: 1024,
    detail: null,
  },
  managed_tenants: 1,
  host_available: true,
  requests: { total: 2, active: 0, warning: 0, error: 0, bytes: 1024 },
} satisfies OverviewData;

const topology = { tenants: [] } satisfies TopologyData;

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
      ["GitHub repo", "https://github.com/rbee3u/aibox", "github"],
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
      const link = within(modules).getByRole("link", { name: new RegExp(`^${label}`) });
      expect(link.querySelector(`[data-icon="${iconName}"]`)).toHaveClass(iconClass);
      expect(link).toHaveAttribute("href", `/_aibox/ui/${iconName}`);
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
    expect(screen.getByRole("link", { name: "GitHub repo" })).toHaveAttribute(
      "title",
      "GitHub repo",
    );
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
    for (const option of ["System", "Light", "Dark"]) {
      expect(within(menu).getByRole("menuitemradio", { name: option })).toHaveAttribute(
        "class",
        expect.stringContaining("themeOption"),
      );
    }

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
    await user.click(screen.getByRole("link", { name: /Overview/ }));
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

    await user.click(screen.getByRole("link", { name: /Overview/ }));

    await screen.findByRole("region", { name: "Service status" });
    expect(screen.getByRole("button", { name: "Color theme: Light" })).toBeInTheDocument();
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });

  it("confirms before leaving a dirty Current Config editor", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/configs?tenant=managed%3Adefault&agent=codex&current=1&file=config.toml",
    );
    mockConfigControlApi();
    const user = userEvent.setup();
    render(<App />);

    const editor = await screen.findByRole("textbox", { name: "config.toml content" });
    await user.type(editor, "changed");
    await waitFor(() => expect(editor).toHaveValue('model = "test"\nchanged'));

    await user.click(screen.getByRole("link", { name: /Overview/ }));
    const dialog = await screen.findByRole("dialog", {
      name: "Discard unsaved Config changes?",
    });
    expect(dialog).toHaveTextContent("Your unsaved Config changes will be lost if you continue.");
    expect(window.location.pathname).toBe("/_aibox/ui/configs");
    expect(editor).toHaveValue('model = "test"\nchanged');

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Discard unsaved Config changes?" })).toBeNull();
    expect(window.location.pathname).toBe("/_aibox/ui/configs");
    expect(screen.getByRole("textbox", { name: "config.toml content" })).toHaveValue(
      'model = "test"\nchanged',
    );

    await user.click(screen.getByRole("link", { name: /Overview/ }));
    await user.click(
      within(
        await screen.findByRole("dialog", { name: "Discard unsaved Config changes?" }),
      ).getByRole("button", { name: "Discard and continue" }),
    );
    await screen.findByRole("region", { name: "Service status" });
    expect(window.location.pathname).toBe("/_aibox/ui/overview");
  });

  it("treats mobile navigation as an inert focus-managed drawer", async () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn().mockReturnValue({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    );
    mockControlApi();
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("region", { name: "Service status" });
    const navigation = screen.getByLabelText("Console navigation", { selector: "aside" });
    const menu = screen.getByRole("button", { name: "Open navigation" });
    expect(navigation).toHaveAttribute("aria-hidden", "true");
    expect(navigation).toHaveProperty("inert", true);

    await user.click(menu);
    expect(navigation).not.toHaveAttribute("aria-hidden");
    expect(navigation).toHaveProperty("inert", false);
    await waitFor(() => expect(screen.getByRole("link", { name: /Overview/ })).toHaveFocus());

    const themeTrigger = within(navigation).getByRole("button", { name: "Color theme: System" });
    await user.click(themeTrigger);
    expect(screen.getByRole("menuitemradio", { name: "System" })).toHaveFocus();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu", { name: "Color theme" })).not.toBeInTheDocument();
    expect(navigation).not.toHaveAttribute("aria-hidden");
    expect(navigation).toHaveProperty("inert", false);
    expect(themeTrigger).toHaveFocus();

    await user.keyboard("{Escape}");
    expect(navigation).toHaveAttribute("aria-hidden", "true");
    expect(navigation).toHaveProperty("inert", true);
    await waitFor(() => expect(menu).toHaveFocus());
  });
});

function mockControlApi() {
  const api = materializeControlApi({
    bootstrap: { version: "1.2.3", csrf_token: "token" },
    get: vi.fn((path: string) => {
      if (path === "/_aibox/api/operations/current") return Promise.resolve({ operation: null });
      if (path === "/_aibox/api/overview") return Promise.resolve(overview);
      if (path === "/_aibox/api/topology") return Promise.resolve(topology);
      return Promise.reject(new Error(`Unexpected Control API request: ${path}`));
    }),
  });
  vi.spyOn(ControlApi, "connect").mockResolvedValue(api);
  vi.stubGlobal(
    "EventSource",
    class {
      addEventListener() {}
      close() {}
    },
  );
}

function mockConfigControlApi() {
  const tenants = [
    {
      kind: "managed",
      name: "default",
      display_name: "default",
      home: "/tmp/aibox/tenants/default",
      exists: true,
    },
  ] satisfies TenantRow[];
  const catalog = {
    configs: [],
    files: ["config.toml", "auth.json"],
    application: { last_application: null, drift: "untracked" },
    credential_propagation_available: false,
  } satisfies ConfigListData;
  const api = materializeControlApi({
    bootstrap: { version: "1.2.3", csrf_token: "token" },
    get: vi.fn((path: string) => {
      if (path === "/_aibox/api/operations/current") return Promise.resolve({ operation: null });
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path === "/_aibox/api/configs?tenant=managed%3Adefault&agent=codex")
        return Promise.resolve(catalog);
      if (path === "/_aibox/api/overview") return Promise.resolve(overview);
      if (path === "/_aibox/api/topology") return Promise.resolve(topology);
      return Promise.reject(new Error(`Unexpected Control API request: ${path}`));
    }),
    post: vi.fn((path: string) => {
      if (path === "/_aibox/api/configs/reveal")
        return Promise.resolve({
          file: "config.toml",
          exists: true,
          revision: "config.toml-revision",
          content_base64: btoa('model = "test"\n'),
        });
      return Promise.reject(new Error(`Unexpected Control API request: ${path}`));
    }),
  });
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
