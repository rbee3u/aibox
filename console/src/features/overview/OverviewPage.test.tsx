import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Operation } from "@/api/operations";
import type { OverviewApi, OverviewData, TopologyData } from "@/api/overview";
import type { SessionSummaryData } from "@/api/sessions";
import { OverviewPage } from "@/features/overview/OverviewPage";
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
    loadSessionSummary: vi.fn(() => Promise.resolve(summary)),
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
  it("renders operational facts and the merged topology toolbar", async () => {
    const api = fakeApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    expect(await screen.findByRole("heading", { name: "Key facts" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Resource topology" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Expand all" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse all" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh topology" })).toBeInTheDocument();
    expect(screen.queryByPlaceholderText("Filter resources")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Topology view options" })).not.toBeInTheDocument();
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
    expect(within(tree).getByText("~")).toBeVisible();
    expect(within(tree).getByText("~/.aibox/tenants/default")).toBeVisible();
    expect(within(tree).queryByText("/home/test/.aibox/tenants/default")).not.toBeInTheDocument();
  });

  it("expands default to Agent resource summaries without a Sessions disclosure", async () => {
    const tree = await openTreeWithApi();
    expect(within(tree).getByText("Codex")).toBeVisible();
    expect(within(tree).getByText("Claude")).toBeVisible();
    expect(within(tree).getAllByText("Current Config")).toHaveLength(2);
    expect(within(tree).getAllByText("Named Configs")).toHaveLength(2);
    expect(within(tree).getAllByText("Sessions")).toHaveLength(2);
    expect(within(tree).queryByRole("button", { name: "Expand Sessions" })).not.toBeInTheDocument();
    expect(within(tree).queryByText("broken")).not.toBeInTheDocument();
    expect(within(tree).getByText("Last applied daily · Clean")).toHaveAttribute(
      "title",
      "Last applied daily · Clean",
    );
    expect(within(tree).getAllByText("Load count on demand")[0]).toHaveAttribute(
      "title",
      "Load count on demand",
    );
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

  it("opens and toggles an anchored node inspector, loading Sessions on demand", async () => {
    const api = fakeApi();
    const user = userEvent.setup();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    const tree = await openTree();
    const sessions = within(tree).getAllByText("Sessions")[0];
    await user.click(sessions);
    const inspector = await screen.findByRole("complementary", { name: "Sessions details" });
    expect(within(inspector).getByText("Neutral")).toBeInTheDocument();
    await waitFor(() => expect(api.loadSessionSummary).toHaveBeenCalledTimes(1));
    await user.click(within(tree).getAllByText("Sessions")[0]);
    expect(
      screen.queryByRole("complementary", { name: "Sessions details" }),
    ).not.toBeInTheDocument();
  });

  it("keeps the inspector outside the topology layout and closes on Escape or outside click", async () => {
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
