import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { OverviewPage } from "./OverviewPage";
import styles from "./OverviewPage.module.css";
import type { Operation, OverviewData, SessionSummaryData, TopologyData } from "./controlApi";
import { ControlApi } from "./controlApi";

const overview = {
  service: {
    version: "1.2.3",
    listen: "127.0.0.1:8080",
    uptime_seconds: 90,
    aibox_root: "/var/lib/aibox",
  },
  docker: { status: "available", error: null },
  runtime_image: {
    reference: "aibox:test",
    status: "built",
    id: "sha256:0123456789abcdef",
    created_at: "2026-08-18T01:02:03Z",
    size_bytes: 4_194_304,
    detail: null,
  },
  managed_tenants: 1,
  host_available: true,
  requests: { total: 7, active: 1, warning: 2, error: 0, bytes: 4096 },
} satisfies OverviewData;

const topology = {
  tenants: [
    {
      kind: "managed",
      name: "default",
      display_name: "default",
      home: "/var/lib/aibox/tenants/default",
      exists: true,
      agents: [
        {
          agent: "codex",
          current_config: { present_files: 2, expected_files: 2 },
          named_configs: {
            entries: [
              { name: "daily", state: "ready" },
              { name: "broken", state: "incomplete", detail: "auth.json is missing" },
            ],
          },
          application: {
            last_application: { applied: "daily", applied_at: "2026-08-18T02:00:00Z" },
            drift: "clean",
          },
        },
        {
          agent: "claude",
          current_config: { present_files: 0, expected_files: 1 },
          named_configs: { entries: [] },
          application: { last_application: null, drift: "untracked" },
        },
      ],
      components: {
        entries: [
          {
            kind: "rust",
            supports_version: true,
            status: "installed",
            version: "1.89.0",
            error: null,
          },
          {
            kind: "go",
            supports_version: true,
            status: "not-installed",
            version: null,
            error: null,
          },
        ],
      },
    },
  ],
} satisfies TopologyData;

const topologyWithTenantOrder = {
  tenants: [
    {
      ...topology.tenants[0],
      name: "studio",
      display_name: "studio",
      home: "/var/lib/aibox/tenants/studio",
    },
    topology.tenants[0],
    {
      ...topology.tenants[0],
      kind: "host",
      name: null,
      display_name: "Host Tenant",
      home: "/home/aibox",
    },
  ],
} satisfies TopologyData;

const operation = {
  id: "operation-1",
  kind: "build image",
  state: "running",
  started_at: "2026-08-18T03:00:00Z",
  ended_at: null,
  result: null,
  first_sequence: 0,
  next_sequence: 0,
  logs: [],
} satisfies Operation;

function fakeApi(topologyData: TopologyData = topology) {
  const sessionSummary = { count: 3, warnings: [], partial: false } satisfies SessionSummaryData;
  const get = vi.fn((path: string) => {
    if (path === "/_aibox/api/overview") return Promise.resolve(overview);
    if (path === "/_aibox/api/topology") return Promise.resolve(topologyData);
    if (path.startsWith("/_aibox/api/sessions/summary?")) return Promise.resolve(sessionSummary);
    return Promise.reject(new Error(`Unexpected GET ${path}`));
  });
  const post = vi.fn(() => Promise.resolve(operation));
  return { api: { get, post } as unknown as ControlApi, get, post };
}

async function openTopology() {
  return screen.findByRole("tree", { name: "Tenant resource topology" });
}

describe("OverviewPage", () => {
  it("shows health, Runtime Image metadata, and both explicit build modes", async () => {
    const { api, post } = fakeApi();
    const onOperation = vi.fn();
    const user = userEvent.setup();
    render(
      <OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={onOperation} />,
    );

    expect(await screen.findByRole("heading", { name: "Key facts" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Runtime" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Resource topology" })).toBeInTheDocument();
    expect(screen.getByText("0123456789ab")).toBeInTheDocument();
    expect(screen.getByText("4.0 MiB")).toBeInTheDocument();
    expect(screen.getAllByText("Docker")).toHaveLength(1);
    expect(screen.getAllByText("Runtime Image")).toHaveLength(1);
    expect(screen.queryByRole("heading", { name: "Storage" })).not.toBeInTheDocument();
    expect(screen.queryByText(/Rebuild/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^Build$/ }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/operations/build", { force: false }),
    );
    await user.click(screen.getByRole("button", { name: "Build without cache" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/operations/build", { force: true }),
    );
    expect(onOperation).toHaveBeenCalledTimes(2);
  });

  it("shows and describes why Runtime Image builds are unavailable", async () => {
    const get = vi.fn((path: string): Promise<unknown> => {
      if (path === "/_aibox/api/overview")
        return Promise.resolve({
          ...overview,
          docker: { status: "unavailable" as const, error: "Docker daemon is offline" },
        });
      if (path === "/_aibox/api/topology") return Promise.resolve(topology);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const api = { get, post: vi.fn() } as unknown as ControlApi;

    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    const reason = await screen.findByText("Docker daemon is offline", { selector: "p" });
    const build = screen.getByRole("button", { name: "Build" });
    const noCache = screen.getByRole("button", { name: "Build without cache" });
    expect(build).toBeDisabled();
    expect(noCache).toBeDisabled();
    expect(build).toHaveAccessibleDescription(reason.textContent ?? "");
    expect(noCache).toHaveAccessibleDescription(reason.textContent ?? "");
  });

  it("routes attention items to the first affected resource", async () => {
    const { api } = fakeApi();
    const onNavigate = vi.fn();
    const user = userEvent.setup();
    render(
      <OverviewPage api={api} operation={null} onNavigate={onNavigate} onOperation={vi.fn()} />,
    );

    const configDetail = await screen.findByText("1 Named Config needs attention.");
    await user.click(configDetail.closest("button")!);

    expect(onNavigate).toHaveBeenCalledWith(
      "configs",
      new URLSearchParams("scope=managed%3Adefault&agent=codex&config=broken"),
    );

    const requestDetail = screen.getByText("0 errors · 2 warnings");
    await user.click(requestDetail.closest("button")!);
    expect(onNavigate).toHaveBeenLastCalledWith("requests", undefined);
  });

  it("shows the complete structural map and loads Session counts on demand", async () => {
    const { api, get } = fakeApi();
    const onNavigate = vi.fn();
    const user = userEvent.setup();
    render(
      <OverviewPage api={api} operation={null} onNavigate={onNavigate} onOperation={vi.fn()} />,
    );

    const tree = await openTopology();
    expect(within(tree).getByText("daily")).toBeVisible();
    expect(within(tree).getByText("broken")).toBeVisible();
    expect(within(tree).getByText("Rust")).toBeVisible();
    expect(within(tree).queryByText("Go")).not.toBeInTheDocument();

    await user.click(screen.getAllByRole("button", { name: "Expand Sessions" })[0]);
    expect((await within(tree).findAllByText("3 Sessions")).length).toBe(2);
    expect(get).toHaveBeenCalledWith(
      "/_aibox/api/sessions/summary?scope=managed&tenant=default&agent=codex",
      expect.any(AbortSignal),
    );

    await user.click(tree.querySelector<HTMLAnchorElement>('a[href*="config=daily"]')!);
    expect(onNavigate).toHaveBeenCalledWith("configs", expect.objectContaining({}));
    const query = onNavigate.mock.calls.at(-1)?.[1] as URLSearchParams;
    expect(query.toString()).toBe("scope=managed%3Adefault&agent=codex&config=daily");
  });

  it("searches hidden branches and supports ARIA tree arrow navigation", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    const tree = await openTopology();
    await user.type(screen.getByRole("searchbox", { name: "Filter topology" }), "rust");
    expect(within(tree).getByText("Rust")).toBeVisible();
    const dailyNode = within(tree).getByText("daily").closest("[data-node-id]");
    expect(dailyNode).toHaveClass(styles.nodeDimmed);

    const search = screen.getByRole("searchbox", { name: "Filter topology" });
    await user.clear(search);
    await user.type(search, "does-not-exist");
    expect(within(tree).getByText("No resources match this filter")).toBeVisible();
    expect(within(tree).queryByText("daily")).not.toBeInTheDocument();

    await user.clear(search);
    const root = within(tree).getByRole("treeitem", { name: /aibox Service/ });
    root.focus();
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toHaveAttribute("role", "treeitem");
    expect(document.activeElement).toHaveAttribute("aria-level", "2");
    expect(document.activeElement).toHaveTextContent("default");
  });

  it("uses the shared module and resource icons throughout the topology", async () => {
    const { api } = fakeApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    const tree = await openTopology();
    expect(tree.querySelector('[data-icon="service"] svg')).toHaveClass("lucide-box");
    expect(tree.querySelector('[data-icon="configs"] svg')).toHaveClass("lucide-file-sliders");
    expect(tree.querySelector('[data-icon="sessions"] svg')).toHaveClass("lucide-messages-square");
    expect(tree.querySelector('[data-icon="config"] svg')).toHaveClass("lucide-file-code-corner");
  });

  it("lays out a connected left-to-right tree across distinct levels", async () => {
    const { api } = fakeApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    const tree = await openTopology();
    const nodes = [...tree.querySelectorAll<HTMLElement>("[data-node-id]")];
    const edges = document.querySelectorAll<SVGPathElement>("[data-edge]");
    expect(nodes.length).toBeGreaterThan(6);
    expect(edges).toHaveLength(nodes.length - 1);
    expect(document.querySelector('[data-edge="service->tenant:managed:default"]')).not.toBeNull();

    const service = tree.querySelector<HTMLElement>('[data-node-id="service"]')!;
    const tenant = tree.querySelector<HTMLElement>('[data-node-id="tenant:managed:default"]')!;
    const codex = tree.querySelector<HTMLElement>(
      '[data-node-id="tenant:managed:default/agent:codex"]',
    )!;
    const claude = tree.querySelector<HTMLElement>(
      '[data-node-id="tenant:managed:default/agent:claude"]',
    )!;
    expect(parseFloat(service.style.left)).toBeLessThan(parseFloat(tenant.style.left));
    expect(parseFloat(tenant.style.left)).toBeLessThan(parseFloat(codex.style.left));
    expect(codex.style.top).not.toBe(claude.style.top);
  });

  it("orders Host first and expands every structural Tenant branch", async () => {
    const { api } = fakeApi(topologyWithTenantOrder);
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    const tree = await openTopology();
    const tenants = within(tree)
      .getAllByRole("treeitem")
      .filter((node) => node.getAttribute("aria-level") === "2");
    expect(tenants.map((node) => node.getAttribute("data-node-id"))).toEqual([
      "tenant:host",
      "tenant:managed:default",
      "tenant:managed:studio",
    ]);
    expect(tenants[0]).toHaveAttribute("aria-expanded", "true");
    expect(tenants[1]).toHaveAttribute("aria-expanded", "true");
    expect(tenants[2]).toHaveAttribute("aria-expanded", "true");
    expect(within(tree).getAllByText("Codex")).toHaveLength(3);
    expect(within(tree).getAllByText("Claude")).toHaveLength(3);
    expect(within(tree).getAllByText("Components")).toHaveLength(3);
    expect(within(tree).getAllByText("Current Config")).toHaveLength(6);
    expect(within(tree).getAllByText("Named Configs")).toHaveLength(6);
  });

  it("zooms in fixed steps, resets to 100%, and fits the topology", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    await openTopology();
    const reset = await screen.findByRole("button", {
      name: /Reset topology zoom to 100%/,
    });
    await waitFor(() => expect(reset).toHaveTextContent("80%"));
    await user.click(reset);
    expect(reset).toHaveTextContent("100%");
    await user.click(screen.getByRole("button", { name: "Zoom out" }));
    expect(reset).toHaveTextContent("90%");
    await user.click(screen.getByRole("button", { name: "Fit topology to width" }));
    expect(reset).toHaveTextContent("80%");
  });

  it("keeps zoom controls in the sticky toolbar and only scrolls the topology horizontally", async () => {
    const { api } = fakeApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    await openTopology();
    const zoomControls = screen.getByLabelText("Topology zoom controls");
    const viewport = document.querySelector<HTMLElement>("[data-topology-viewport]")!;
    const page = document.querySelector<HTMLElement>("[data-overview-scroll]")!;
    const frame = viewport.firstElementChild as HTMLElement;
    expect(zoomControls.parentElement).toHaveClass(styles.topologyToolbar);
    expect(document.querySelector(`.${styles.canvasShell} .${styles.zoomControls}`)).toBeNull();
    expect(page).toContainElement(viewport);
    expect(page).toHaveAttribute("data-scroll-axis", "vertical");
    expect(viewport).toHaveAttribute("data-scroll-axis", "horizontal");
    expect(parseFloat(frame.style.height)).toBeGreaterThan(0);
  });

  it("starts narrow topology layouts at 100% without forcing Fit mode", async () => {
    const { api } = fakeApi();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(private readonly callback: ResizeObserverCallback) {}
        observe() {
          this.callback([{ contentRect: { width: 600 } } as ResizeObserverEntry], this);
        }
        disconnect() {}
        unobserve() {}
      },
    );
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    await openTopology();
    const zoom = await screen.findByRole("button", { name: /Reset topology zoom to 100%/ });
    await waitFor(() => expect(zoom).toHaveTextContent("100%"));
    expect(screen.getByRole("button", { name: "Fit topology to width" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("refits structural changes only while Fit mode remains active", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    await openTopology();
    const zoom = await screen.findByRole("button", { name: /Reset topology zoom to 100%/ });
    await waitFor(() => expect(zoom).toHaveTextContent("80%"));
    await user.click(screen.getByRole("button", { name: "Collapse Codex" }));
    await waitFor(() => expect(zoom).toHaveTextContent("100%"));
    await user.click(screen.getByRole("button", { name: "Expand Codex" }));
    await waitFor(() => expect(zoom).toHaveTextContent("80%"));

    await user.click(zoom);
    await user.click(screen.getByRole("button", { name: "Collapse Codex" }));
    expect(zoom).toHaveTextContent("100%");
    await user.click(screen.getByRole("button", { name: "Expand Codex" }));
    expect(zoom).toHaveTextContent("100%");
  });

  it("opens diagnostic details and closes them with Escape", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    await openTopology();
    await user.click(screen.getByRole("button", { name: "Show details for broken" }));
    expect(screen.getByRole("tooltip")).toHaveTextContent("auth.json is missing");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("keeps the operated branch as the roving focus anchor when it is collapsed", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    const tree = await openTopology();
    const daily = tree.querySelector<HTMLElement>(
      '[data-node-id="tenant:managed:default/agent:codex/named-configs/daily"]',
    )!;
    daily.focus();
    await waitFor(() => expect(daily).toHaveAttribute("tabindex", "0"));

    await user.click(screen.getByRole("button", { name: "Collapse Codex" }));
    const codex = tree.querySelector<HTMLElement>(
      '[data-node-id="tenant:managed:default/agent:codex"]',
    )!;
    expect(codex).toHaveAttribute("tabindex", "0");
    await user.click(screen.getByRole("button", { name: "Expand Codex" }));
    expect(codex).toHaveAttribute("tabindex", "0");
    expect(
      tree.querySelector('[data-node-id="tenant:managed:default/agent:codex/named-configs/daily"]'),
    ).toHaveAttribute("tabindex", "-1");
  });

  it("compensates branch reflow on the Overview page without vertical canvas scrolling", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    const tree = await openTopology();
    const reset = await screen.findByRole("button", { name: /Reset topology zoom to 100%/ });
    await waitFor(() => expect(reset).toHaveTextContent("80%"));
    await user.click(reset);
    const page = document.querySelector<HTMLElement>("[data-overview-scroll]")!;
    const viewport = document.querySelector<HTMLElement>("[data-topology-viewport]")!;
    const codex = tree.querySelector<HTMLElement>(
      '[data-node-id="tenant:managed:default/agent:codex"]',
    )!;
    let measurement = 0;
    vi.spyOn(codex, "getBoundingClientRect").mockImplementation(() =>
      testRect(measurement++ === 0 ? 100 : 125, measurement === 1 ? 300 : 360),
    );
    page.scrollTop = 200;
    viewport.scrollLeft = 20;

    await user.click(screen.getByRole("button", { name: "Collapse Codex" }));
    await waitFor(() => expect(page.scrollTop).toBe(260));
    expect(viewport.scrollLeft).toBe(45);
    expect(viewport.scrollTop).toBe(0);
  });

  it("anchors manual zoom changes on the active topology node", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);

    const tree = await openTopology();
    const reset = await screen.findByRole("button", { name: /Reset topology zoom to 100%/ });
    await waitFor(() => expect(reset).toHaveTextContent("80%"));
    await user.click(reset);
    const daily = tree.querySelector<HTMLElement>(
      '[data-node-id="tenant:managed:default/agent:codex/named-configs/daily"]',
    )!;
    daily.focus();
    await waitFor(() => expect(daily).toHaveAttribute("tabindex", "0"));
    const page = document.querySelector<HTMLElement>("[data-overview-scroll]")!;
    const viewport = document.querySelector<HTMLElement>("[data-topology-viewport]")!;
    page.scrollTop = 500;
    viewport.scrollLeft = 200;

    await user.click(screen.getByRole("button", { name: "Zoom out" }));
    expect(page.scrollTop).toBeLessThan(500);
    expect(viewport.scrollLeft).toBeLessThan(200);
  });
});

function testRect(left: number, top: number): DOMRect {
  return {
    x: left,
    y: top,
    left,
    top,
    right: left + 184,
    bottom: top + 58,
    width: 184,
    height: 58,
    toJSON: () => ({}),
  };
}
