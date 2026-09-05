import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Operation } from "@/api/operations";
import type { OverviewApi, OverviewData, TopologyData } from "@/api/overview";
import type { SessionSummaryData } from "@/api/sessions";
import { OverviewPage } from "@/features/overview/OverviewPage";
import overviewStyles from "@/features/overview/OverviewPage.module.css";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import actionStyles from "@/shared/ui/ActionButton.module.css";
import { deferred } from "@/test/deferred";

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
  host_home: "/home/test",
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
            count: 2,
            attention: [{ name: "broken", state: "incomplete", detail: "auth.json is missing" }],
          },
          application: {
            last_application: { applied: "daily", applied_at: "2026-08-18T02:00:00Z" },
            drift: "clean",
          },
        },
        {
          agent: "claude",
          current_config: { present_files: 1, expected_files: 1 },
          named_configs: { count: 0, attention: [] },
          application: { last_application: null, drift: "untracked" },
        },
      ],
      components: { total: 2, installed: 1, attention: [] },
    },
  ],
} satisfies TopologyData;

const quietTopology = {
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
          named_configs: { count: 2, attention: [] },
          application: {
            last_application: { applied: "daily", applied_at: "2026-08-18T02:00:00Z" },
            drift: "clean",
          },
        },
      ],
      components: { total: 2, installed: 2, attention: [] },
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

function fakeApi(data: TopologyData = topology) {
  const summary = { count: 3, warnings: [], partial: false } satisfies SessionSummaryData;
  const api = {
    loadOverview: vi.fn((): Promise<OverviewData> => Promise.resolve(overview)),
    loadTopology: vi.fn((): Promise<TopologyData> => Promise.resolve(data)),
    loadSessionSummary: vi.fn((): Promise<SessionSummaryData> => Promise.resolve(summary)),
    buildImage: vi.fn(() => Promise.resolve(operation)),
  } satisfies OverviewApi;
  return api;
}

function delayedApi(options?: { overviewData?: OverviewData; topologyData?: TopologyData }) {
  const overviewLoad = deferred<OverviewData>();
  const topologyLoad = deferred<TopologyData>();
  const api = fakeApi(options?.topologyData ?? quietTopology);
  api.loadOverview.mockImplementation(() => overviewLoad.promise);
  api.loadTopology.mockImplementation(() => topologyLoad.promise);
  return {
    api,
    resolveOverview: () => overviewLoad.resolve(options?.overviewData ?? overview),
    resolveTopology: () => topologyLoad.resolve(options?.topologyData ?? quietTopology),
  };
}

function attentionRegion() {
  return screen.getByRole("region", { name: "Needs attention" });
}

function expectAttentionPending() {
  const region = attentionRegion();
  expect(within(region).getByRole("status")).toHaveTextContent("Inspecting service and topology");
  expect(
    within(region).queryByText("No warnings or errors are currently reported."),
  ).not.toBeInTheDocument();
}

async function openTree() {
  return screen.findByRole("tree", { name: "Tenant resource topology" });
}

describe("OverviewPage", () => {
  it("renders a quiet status strip and the merged topology toolbar", async () => {
    const api = fakeApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    expect(await screen.findByRole("region", { name: "Service status" })).toBeInTheDocument();
    expect(
      await screen.findByText(
        "Running · 1 Managed · Host available · Docker available · Image built",
      ),
    ).toBeInTheDocument();
    expect(document.querySelector("[data-overview-status-summary]")).toHaveAttribute(
      "title",
      "1.2.3 · 127.0.0.1:8080 · /var/lib/aibox",
    );
    expect(screen.getByRole("region", { name: "Needs attention" })).toBeInTheDocument();
    expect(screen.queryByText("Attention summary")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Needs attention" })).toHaveClass("srOnly");
    const facts = document.querySelector("[data-overview-status-facts]");
    const meta = document.querySelector("[data-overview-status-meta]");
    expect(facts).toHaveTextContent("Service");
    expect(facts).toHaveTextContent("Docker");
    expect(facts).toHaveTextContent("Runtime Image");
    expect(facts).not.toHaveTextContent("Version");
    expect(meta).toHaveTextContent("Version");
    expect(meta).toHaveTextContent("1.2.3");
    expect(meta).toHaveTextContent("127.0.0.1:8080");
    expect(meta).toHaveTextContent("/var/lib/aibox");
    expect(document.querySelectorAll("[data-overview-meta]")).toHaveLength(3);
    expect(screen.getByRole("heading", { name: "Resource topology" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Expand all" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse all" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh topology" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Build" })).toHaveClass(actionStyles.secondary);
    expect(screen.getByRole("button", { name: "More build options" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Build without cache" })).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText("Filter resources")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Topology view options" })).not.toBeInTheDocument();
  });

  it("makes Build the primary action only while the Runtime Image is missing", async () => {
    const api = fakeApi();
    api.loadOverview.mockResolvedValue({
      ...overview,
      runtime_image: { ...overview.runtime_image, status: "missing", id: null, detail: null },
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    expect(await screen.findByRole("button", { name: "Build" })).toHaveClass(
      actionStyles.primarySoft,
    );
    expect(screen.getByRole("button", { name: "Build without cache" })).toHaveClass(
      actionStyles.secondary,
    );
    expect(screen.queryByRole("button", { name: "More build options" })).not.toBeInTheDocument();
  });

  it("keeps the cacheless rebuild behind Build options while the image is Built", async () => {
    const user = userEvent.setup();
    const api = fakeApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    expect(await screen.findByRole("button", { name: "Build" })).toHaveClass(
      actionStyles.secondary,
    );
    await user.click(screen.getByRole("button", { name: "More build options" }));
    await user.click(screen.getByRole("menuitem", { name: "Build without cache" }));
    expect(api.buildImage).toHaveBeenCalledWith(true);
  });

  it("abbreviates AIBox Root and Tenant homes under the Host Home", async () => {
    const api = fakeApi({
      tenants: [
        {
          kind: "host",
          name: null,
          display_name: "Host Tenant",
          home: "/home/test",
          exists: true,
          agents: [],
          components: { total: 0, installed: 0, attention: [] },
        },
        {
          kind: "managed",
          name: "default",
          display_name: "default",
          home: "/home/test/.aibox/tenants/default",
          exists: true,
          agents: [],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    });
    api.loadOverview.mockResolvedValue({
      ...overview,
      service: { ...overview.service, aibox_root: "/home/test/.aibox" },
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    const root = await screen.findByText("~/.aibox");
    expect(root).toHaveAttribute("title", "/home/test/.aibox");
    expect(root.closest("div")).toHaveTextContent("AIBox Root");
    const tree = await openTree();
    const managed = within(tree).getByRole("treeitem", {
      name: "default ~/.aibox/tenants/default",
    });
    expect(within(tree).getByText("~")).toBeVisible();
    expect(within(managed).getByText("~/.aibox/tenants/default")).toBeVisible();
    expect(managed).toHaveAccessibleDescription("/home/test/.aibox/tenants/default");
    expect(
      within(managed).queryByText("/home/test/.aibox/tenants/default", { selector: "small" }),
    ).not.toBeInTheDocument();
  });

  it("expands the first Config attention Agent without a Sessions disclosure", async () => {
    const user = userEvent.setup();
    const tree = await openTreeWithApi();
    expect(within(tree).getByText("Codex")).toBeVisible();
    expect(within(tree).getByText("Claude")).toBeVisible();
    expect(within(tree).getAllByText("Current Config")).toHaveLength(1);
    expect(within(tree).getAllByText("Named Configs")).toHaveLength(1);
    expect(within(tree).getAllByText("Sessions")).toHaveLength(1);
    expect(within(tree).queryByRole("button", { name: "Expand Sessions" })).not.toBeInTheDocument();
    expect(within(tree).queryByText("broken")).not.toBeInTheDocument();
    expect(within(tree).getByText("daily · Clean")).toBeVisible();
    expect(within(tree).getByText("Load count")).toBeVisible();
    const sessions = within(tree).getAllByRole("treeitem", { name: /Sessions Load count/ })[0];
    expect(sessions).toHaveAccessibleDescription("Load count on demand");
    await user.hover(sessions);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Load count on demand");
    await user.unhover(sessions);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("discloses truncated node copy when the treeitem is keyboard-focused", async () => {
    const tree = await openTreeWithApi();
    const sessions = within(tree).getAllByRole("treeitem", { name: /Sessions Load count/ })[0];
    const matches = sessions.matches.bind(sessions);
    vi.spyOn(sessions, "matches").mockImplementation((selector) =>
      selector === ":focus-visible" ? true : matches(selector),
    );
    sessions.focus();
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Load count on demand");
  });

  it("expands the first Component attention Tenant instead of the healthy default Tenant", async () => {
    const data = {
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
              named_configs: { count: 0, attention: [] },
              application: { last_application: null, drift: "clean" },
            },
          ],
          components: { total: 1, installed: 1, attention: [] },
        },
        {
          kind: "managed",
          name: "shadow1",
          display_name: "shadow1",
          home: "/var/lib/aibox/tenants/shadow1",
          exists: true,
          agents: [],
          components: {
            total: 1,
            installed: 1,
            attention: [
              {
                kind: "claude-statusline",
                supports_version: false,
                status: "modified",
                version: null,
                error: null,
              },
            ],
          },
        },
      ],
    } satisfies TopologyData;
    render(
      <OverviewPage
        api={fakeApi(data)}
        operation={null}
        onNavigate={vi.fn()}
        onOperation={vi.fn()}
      />,
    );
    const tree = await openTree();
    expect(within(tree).getByText("Claude Statusline")).toBeVisible();
    expect(within(tree).queryByText("Codex")).not.toBeInTheDocument();
  });

  it("keeps treeitem focus after a click so arrow keys move between nodes", async () => {
    const user = userEvent.setup();
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />,
    );
    const tree = await openTree();
    const named = within(tree).getAllByRole("treeitem", { name: /Named Configs/ })[0];
    await user.click(within(named).getByText("Named Configs"));
    expect(named).toHaveFocus();
    expect(within(named).queryByRole("button", { name: /^Named Configs/ })).not.toBeInTheDocument();
    await user.keyboard("{ArrowDown}");
    expect(within(tree).getAllByRole("treeitem", { name: /Sessions/ })[0]).toHaveFocus();
  });

  it("gives catalog nodes the same quiet card chrome as identity nodes", async () => {
    const tree = await openTreeWithApi();
    const identity = tree.querySelector<HTMLElement>(
      `[data-node-kind="entity"] .${overviewStyles.nodeSurface}`,
    );
    const catalog = tree.querySelector<HTMLElement>(
      `[data-node-kind="group"] .${overviewStyles.nodeSurface}, [data-node-kind="leaf"] .${overviewStyles.nodeSurface}`,
    );
    expect(identity).toBeTruthy();
    expect(catalog).toBeTruthy();
    expect(identity).toHaveClass(overviewStyles.nodeSurface);
    expect(catalog).toHaveClass(overviewStyles.nodeSurface);
  });

  it("opens and toggles a docked node inspector, loading Sessions on demand", async () => {
    const api = fakeApi();
    const summary = deferred<SessionSummaryData>();
    api.loadSessionSummary.mockImplementation(() => summary.promise);
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    const tree = await openTree();
    const sessions = within(tree).getAllByText("Sessions")[0];
    await user.click(sessions);
    const inspector = await screen.findByRole("complementary", { name: "Sessions details" });
    expect(within(inspector).getByText("Neutral")).toBeInTheDocument();
    expect(
      within(inspector).getByText(/Load on demand|Discovering Transcripts/),
    ).toBeInTheDocument();
    expect(within(inspector).queryByText("Load count")).not.toBeInTheDocument();
    expect(tree.closest("[data-inspector='open']")).toContainElement(inspector);
    await waitFor(() => expect(api.loadSessionSummary).toHaveBeenCalledTimes(1));
    summary.resolve({ count: 3, warnings: [], partial: false });
    await waitFor(() => {
      expect(within(inspector).getByText("3 Sessions")).toBeInTheDocument();
    });
    expect(within(inspector).queryByText("Load on demand")).not.toBeInTheDocument();
    expect(within(inspector).queryByText("Discovering Transcripts")).not.toBeInTheDocument();
    await user.click(within(tree).getAllByText("Sessions")[0]);
    expect(
      screen.queryByRole("complementary", { name: "Sessions details" }),
    ).not.toBeInTheDocument();
  });

  it("docks node details beside the tree so children stay clickable", async () => {
    const user = userEvent.setup();
    const tree = await openTreeWithApi();
    const codex = within(tree).getByRole("treeitem", {
      name: /Codex openai · Dirty|Codex daily · Clean/,
    });
    await user.hover(codex);
    expect(await screen.findByRole("tooltip")).toBeInTheDocument();
    await user.click(codex);
    const inspector = await screen.findByRole("complementary", { name: "Codex details" });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    expect(tree.closest("[aria-labelledby='topology-title']")).toContainElement(inspector);
    expect(inspector).toHaveClass(overviewStyles.topologyInspector);
    expect(within(inspector).getByText("Coding agent")).toBeInTheDocument();
    expect(within(inspector).getByText("Last applied")).toBeInTheDocument();
    expect(within(inspector).getByText("daily · 2026-08-18 10:00:00")).toBeInTheDocument();
    expect(within(inspector).queryByText("daily · Clean")).not.toBeInTheDocument();
    const current = within(tree).getByRole("treeitem", { name: /Current Config/ });
    expect(current).toBeVisible();
    await user.click(current);
    const currentInspector = screen.getByRole("complementary", { name: "Current Config details" });
    expect(within(currentInspector).getByText("Configuration")).toBeInTheDocument();
    expect(within(currentInspector).getByText("Last applied")).toBeInTheDocument();
    expect(within(currentInspector).queryByText("2/2 files present")).not.toBeInTheDocument();
  });

  it("fills the inspector with facts the card shortened, not the card line again", async () => {
    const dirty = {
      ...topology,
      tenants: topology.tenants.map((tenant) => ({
        ...tenant,
        agents: tenant.agents.map((agent) =>
          agent.agent === "codex"
            ? {
                ...agent,
                application: {
                  ...agent.application,
                  drift: "dirty" as const,
                  detail: "differs from last applied",
                },
              }
            : agent,
        ),
      })),
    };
    const user = userEvent.setup();
    render(
      <OverviewPage
        api={fakeApi(dirty)}
        operation={null}
        onNavigate={vi.fn()}
        onOperation={vi.fn()}
      />,
    );
    const tree = await openTree();
    await user.click(within(tree).getByRole("treeitem", { name: /Current Config Dirty/ }));
    const inspector = await screen.findByRole("complementary", { name: "Current Config details" });
    expect(within(inspector).getByText("Needs attention")).toBeInTheDocument();
    expect(within(inspector).getByText("differs from last applied")).toBeInTheDocument();
    expect(within(inspector).getByText("daily · 2026-08-18 10:00:00")).toBeInTheDocument();
    expect(within(inspector).getByText("2/2 present")).toBeInTheDocument();
    expect(within(inspector).queryByText(/^Dirty$/)).not.toBeInTheDocument();
  });

  it("says when Named Configs need no attention instead of repeating the count", async () => {
    const user = userEvent.setup();
    render(
      <OverviewPage
        api={fakeApi(quietTopology)}
        operation={null}
        onNavigate={vi.fn()}
        onOperation={vi.fn()}
      />,
    );
    const tree = await openTree();
    await user.click(within(tree).getByRole("treeitem", { name: /Named Configs 2 Configs/ }));
    const inspector = await screen.findByRole("complementary", { name: "Named Configs details" });
    expect(within(inspector).getByText("None need attention")).toBeInTheDocument();
    expect(within(inspector).queryByText("2 Configs")).not.toBeInTheDocument();
  });

  it("puts a leaf status in the inspector when there is no extra fact", async () => {
    const data = {
      tenants: [
        {
          kind: "managed" as const,
          name: "shadow1",
          display_name: "shadow1",
          home: "/var/lib/aibox/tenants/shadow1",
          exists: true,
          agents: [],
          components: {
            total: 1,
            installed: 1,
            attention: [
              {
                kind: "claude-statusline" as const,
                supports_version: false,
                status: "modified" as const,
                version: null,
                error: null,
              },
            ],
          },
        },
      ],
    } satisfies TopologyData;
    const user = userEvent.setup();
    render(
      <OverviewPage
        api={fakeApi(data)}
        operation={null}
        onNavigate={vi.fn()}
        onOperation={vi.fn()}
      />,
    );
    const tree = await openTree();
    await user.click(within(tree).getByRole("treeitem", { name: /Claude Statusline Modified/ }));
    const inspector = await screen.findByRole("complementary", {
      name: "Claude Statusline details",
    });
    expect(within(inspector).getByText("Needs attention")).toBeInTheDocument();
    expect(within(inspector).getByText("Modified")).toBeInTheDocument();
  });

  it("closes the docked inspector on Escape or outside click", async () => {
    const user = userEvent.setup();
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />,
    );
    const tree = await openTree();
    await user.click(within(tree).getByText("default"));
    expect(
      await screen.findByRole("complementary", { name: "default details" }),
    ).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("complementary", { name: "default details" }),
    ).not.toBeInTheDocument();
    await user.click(within(tree).getByText("default"));
    expect(
      await screen.findByRole("complementary", { name: "default details" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("heading", { name: "Resource topology" }));
    expect(
      screen.queryByRole("complementary", { name: "default details" }),
    ).not.toBeInTheDocument();
  });

  it("does not put a child-count badge on collapsed branches", async () => {
    const user = userEvent.setup();
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />,
    );
    const tree = await openTree();
    await user.click(screen.getByRole("button", { name: "Collapse all" }));
    for (const button of within(tree).getAllByRole("button", { name: /^Expand / })) {
      expect(button.textContent).not.toMatch(/\d/);
    }
  });

  it("expands all containers without requesting Session summaries", async () => {
    const api = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    await openTree();
    await user.click(screen.getByRole("button", { name: "Expand all" }));
    expect(api.loadSessionSummary).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Expand Sessions" })).not.toBeInTheDocument();
  });

  it("uses smooth parent-child curves and vertically centers child subtrees", async () => {
    const user = userEvent.setup();
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />,
    );
    const tree = await openTree();
    const edges = [...document.querySelectorAll<SVGPathElement>("[data-edge]")];
    expect(edges.length).toBeGreaterThan(0);
    expect(edges[0].getAttribute("d")).toContain(" C ");
    const codex = tree.querySelector<HTMLElement>('[data-node-id$="agent:codex"]')!;
    const claude = tree.querySelector<HTMLElement>('[data-node-id$="agent:claude"]')!;
    expect(parseFloat(codex.style.top)).toBeLessThan(parseFloat(claude.style.top));
    await user.click(screen.getByRole("button", { name: "Collapse all" }));
    expect(within(tree).getByText("default")).toBeVisible();
  });

  it("names the first Config attention target and its reason without inventory facts", async () => {
    const onNavigate = vi.fn<ConsoleNavigate>();
    const user = userEvent.setup();
    render(
      <OverviewPage
        api={fakeApi()}
        operation={null}
        onNavigate={onNavigate}
        onOperation={vi.fn()}
      />,
    );
    const attention = await screen.findByRole("region", { name: "Needs attention" });
    const configs = within(attention).getByRole("button", { name: /Configs/ });
    expect(configs).toHaveAttribute("data-overview-attention");
    expect(configs).toHaveClass(overviewStyles.attentionItem);
    expect(configs).toHaveTextContent("default · Codex · broken is incomplete");
    expect(within(attention).queryByText(/Named Config needs attention/i)).not.toBeInTheDocument();
    const facts = [...document.querySelectorAll<HTMLElement>("[data-overview-fact]")];
    expect(facts.some((fact) => /needs attention/i.test(fact.textContent ?? ""))).toBe(false);
    expect(facts.some((fact) => fact.textContent?.includes("Named Configs"))).toBe(false);
    expect(facts.some((fact) => fact.textContent?.includes("Managed Tenants"))).toBe(true);
    const topologyTitle = screen.getByRole("heading", { name: "Resource topology" }).closest("div");
    expect(topologyTitle).toHaveTextContent("1 Managed");
    expect(topologyTitle).not.toHaveTextContent("need attention");
    await user.click(configs);
    expect(onNavigate).toHaveBeenCalledOnce();
    const [module, query] = onNavigate.mock.calls[0] ?? [];
    expect(module).toBe("configs");
    expect(query?.toString()).toBe("tenant=managed%3Adefault&agent=codex&config=broken");
  });

  it("does not claim a healthy attention summary until overview and topology settle", async () => {
    const { api, resolveOverview, resolveTopology } = delayedApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    expectAttentionPending();
    resolveOverview();
    expect(await screen.findByText("Running")).toBeInTheDocument();
    expectAttentionPending();
    resolveTopology();
    expect(
      await screen.findByText("No warnings or errors are currently reported."),
    ).toBeInTheDocument();
    expect(within(attentionRegion()).getByRole("status")).toHaveTextContent(
      "No warnings or errors are currently reported.",
    );
  });

  it("keeps the pending attention copy when topology arrives before overview", async () => {
    const { api, resolveOverview, resolveTopology } = delayedApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    resolveTopology();
    expect(
      await screen.findByRole("tree", { name: "Tenant resource topology" }),
    ).toBeInTheDocument();
    expectAttentionPending();
    resolveOverview();
    expect(
      await screen.findByText("No warnings or errors are currently reported."),
    ).toBeInTheDocument();
  });

  it("shows known service attention before topology settles", async () => {
    const { api, resolveOverview, resolveTopology } = delayedApi({
      overviewData: {
        ...overview,
        docker: { status: "unavailable", error: "Docker is not running" },
      },
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    resolveOverview();
    expect(await screen.findByRole("button", { name: /Docker/ })).toHaveTextContent(
      "Docker is not running",
    );
    expect(
      screen.queryByText("No warnings or errors are currently reported."),
    ).not.toBeInTheDocument();
    expect(within(attentionRegion()).queryByRole("status")).not.toBeInTheDocument();
    resolveTopology();
    expect(await screen.findByRole("button", { name: /Docker/ })).toBeInTheDocument();
  });

  it("returns to pending while a visible status refresh is in flight", async () => {
    const user = userEvent.setup();
    const api = fakeApi(quietTopology);
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    expect(
      await screen.findByText("No warnings or errors are currently reported."),
    ).toBeInTheDocument();
    const refresh = deferred<OverviewData>();
    api.loadOverview.mockImplementationOnce(() => refresh.promise);
    await user.click(screen.getByRole("button", { name: "Refresh status" }));
    expectAttentionPending();
    refresh.resolve(overview);
    expect(
      await screen.findByText("No warnings or errors are currently reported."),
    ).toBeInTheDocument();
  });

  it("returns to pending while a visible topology refresh is in flight", async () => {
    const user = userEvent.setup();
    const api = fakeApi(quietTopology);
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    expect(
      await screen.findByText("No warnings or errors are currently reported."),
    ).toBeInTheDocument();
    const refresh = deferred<TopologyData>();
    api.loadTopology.mockImplementationOnce(() => refresh.promise);
    await user.click(screen.getByRole("button", { name: "Refresh topology" }));
    expectAttentionPending();
    refresh.resolve(quietTopology);
    expect(
      await screen.findByText("No warnings or errors are currently reported."),
    ).toBeInTheDocument();
  });
});

async function openTreeWithApi() {
  render(
    <OverviewPage api={fakeApi()} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />,
  );
  return openTree();
}
