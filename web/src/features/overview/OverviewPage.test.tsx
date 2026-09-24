import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Operation } from "@/api/operations";
import type { OverviewApi, OverviewData, TopologyData } from "@/api/overview";
import { OverviewPage } from "@/features/overview/OverviewPage";
import type { OverviewBrowsingMemory } from "@/features/overview/browsingState";
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
          sessions: { count: 12 },
          application: {
            last_application: { applied: "daily", applied_at: "2026-08-18T02:00:00Z" },
            drift: "clean",
          },
        },
        {
          agent: "claude",
          current_config: { present_files: 1, expected_files: 1 },
          named_configs: { count: 0, attention: [] },
          sessions: { count: 0 },
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
          sessions: { count: 0 },
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
  return {
    loadOverview: vi.fn((): Promise<OverviewData> => Promise.resolve(overview)),
    loadTopology: vi.fn((): Promise<TopologyData> => Promise.resolve(data)),
    buildImage: vi.fn(() => Promise.resolve(operation)),
  } satisfies OverviewApi;
}
function delayedApi() {
  const overviewLoad = deferred<OverviewData>();
  const topologyLoad = deferred<TopologyData>();
  const api = fakeApi(quietTopology);
  api.loadOverview.mockImplementation(() => overviewLoad.promise);
  api.loadTopology.mockImplementation(() => topologyLoad.promise);
  return {
    api,
    resolveOverview: () => overviewLoad.resolve(overview),
    resolveTopology: () => topologyLoad.resolve(quietTopology),
  };
}
function attentionRegion() {
  return screen.getByRole("region", { name: "Needs attention" });
}
function expectAttentionPending() {
  expect(within(attentionRegion()).getByRole("status")).toHaveTextContent(
    "Inspecting service and topology",
  );
  expect(
    screen.queryByText("No warnings or errors are currently reported."),
  ).not.toBeInTheDocument();
}
describe("OverviewPage", () => {
  it("states every Tenant's Codex and Claude summary without asking for a click", async () => {
    const user = userEvent.setup();
    const navigate = vi.fn<ConsoleNavigate>();
    const tenant = topology.tenants[0];
    const api = fakeApi({ tenants: [tenant, { ...tenant, name: "alpha", display_name: "alpha" }] });
    render(<OverviewPage api={api} operation={null} onNavigate={navigate} onOperation={vi.fn()} />);
    const table = await screen.findByRole("table", { name: "Tenant status" });
    expect(
      within(table)
        .getAllByRole("rowheader")
        .map((row) => row.textContent),
    ).toEqual(["defaultManaged Tenant", "alphaManaged Tenant"]);
    expect(within(table).getAllByText("Last applied: daily")).toHaveLength(2);
    expect(within(table).getAllByText("No recorded application")).toHaveLength(2);
    expect(within(table).getAllByText("12 Sessions")).toHaveLength(2);
    expect(within(table).getAllByText("0 Sessions")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: "Topology" })).not.toBeInTheDocument();
    const alpha = within(table).getAllByRole("row")[2];
    await user.click(within(alpha).getByRole("link", { name: "Last applied: daily" }));
    expect(navigate.mock.lastCall?.[1]?.toString()).toBe(
      "tenant=managed%3Aalpha&agent=codex&current=1",
    );
  });
  it("shows unavailable inspections and missing resources explicitly in the default list", async () => {
    const tenant = quietTopology.tenants[0];
    const api = fakeApi({
      tenants: [
        {
          ...tenant,
          exists: false,
          components: { ...tenant.components, installed: 0, error: "Unreadable" },
          agents: [
            {
              ...tenant.agents[0],
              current_config: { ...tenant.agents[0].current_config, error: "Unreadable" },
              named_configs: { count: 0, attention: [], error: "Unreadable" },
            },
          ],
        },
      ],
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    const table = await screen.findByRole("table", { name: "Tenant status" });
    expect(within(table).getByText(/Home unavailable/)).toBeVisible();
    expect(within(table).getByText(/Current Config: Inspection failed/)).toBeVisible();
    expect(within(table).getAllByText(/Catalog inspection failed/)).toHaveLength(2);
    expect(within(table).getByText("Not reported")).toBeVisible();
    expect(within(table).queryByText(/0 Configs|0\/2 installed/)).not.toBeInTheDocument();
  });

  it("shows all Tenants in stable order with Host first and the rest alphabetical", async () => {
    const user = userEvent.setup();
    const tenant = topology.tenants[0];
    const api = fakeApi({
      tenants: [
        { ...tenant, name: "zeta", display_name: "zeta" },
        tenant,
        { ...tenant, kind: "host", name: null, display_name: "Host Tenant" },
        { ...tenant, name: "alpha", display_name: "alpha" },
      ],
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    const table = await screen.findByRole("table", { name: "Tenant status" });
    expect(
      within(table)
        .getAllByRole("rowheader")
        .map((row) => row.querySelector("strong")?.textContent),
    ).toEqual(["Host Tenant", "default", "alpha", "zeta"]);
    await user.click(screen.getByRole("button", { name: "Refresh Overview" }));
    await waitFor(() => expect(api.loadTopology).toHaveBeenCalledTimes(2));
    expect(within(table).getAllByRole("rowheader")).toHaveLength(4);
  });
  it.each(["host", "other", "empty"])("uses the %s fallback", async (kind) => {
    const tenant = quietTopology.tenants[0];
    const api = fakeApi({
      tenants:
        kind === "empty"
          ? []
          : [
              kind === "host"
                ? { ...tenant, kind: "host", name: null, display_name: "Host Tenant" }
                : { ...tenant, name: "other", display_name: "other" },
            ],
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    if (kind === "empty")
      expect(await screen.findByText("No Tenants are currently reported.")).toBeVisible();
    else {
      const table = await screen.findByRole("table", { name: "Tenant status" });
      expect(within(table).getByText(kind === "host" ? "Host Tenant" : "other")).toBeVisible();
    }
  });
  it("restores the scroll position after fresh data arrives", async () => {
    const memory: OverviewBrowsingMemory = { current: null };
    const api = fakeApi();
    const props = {
      api,
      operation: null,
      onNavigate: vi.fn(),
      onOperation: vi.fn(),
      browsingMemory: memory,
    };
    const view = render(<OverviewPage {...props} />);
    const table = await screen.findByRole("table", { name: "Tenant status" });
    const scroller = table.closest<HTMLElement>("[data-overview-scroll]")!;
    scroller.scrollTop = 160;
    view.unmount();
    expect(memory.current?.scrollTop).toBe(160);
    const loading = deferred<TopologyData>();
    api.loadTopology.mockImplementationOnce(() => loading.promise);
    render(<OverviewPage {...props} />);
    loading.resolve(topology);
    const restored = await screen.findByRole("table", { name: "Tenant status" });
    expect(restored.closest<HTMLElement>("[data-overview-scroll]")!.scrollTop).toBe(160);
  });
  it("navigates each resource directly from the Tenant row", async () => {
    const user = userEvent.setup();
    const navigate = vi.fn<ConsoleNavigate>();
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={navigate} onOperation={vi.fn()} />,
    );
    const table = await screen.findByRole("table", { name: "Tenant status" });
    const cases = [
      ["Last applied: daily", "configs", "tenant=managed%3Adefault&agent=codex&current=1"],
      ["2 Named Configs", "configs", "tenant=managed%3Adefault&agent=codex&named=1"],
      ["12 Sessions", "sessions", "tenant=managed%3Adefault&agent=codex"],
      ["Manage components", "tenants", "tenant=managed%3Adefault"],
      ["default", "tenants", "tenant=managed%3Adefault"],
    ];
    for (const [name, module, query] of cases) {
      await user.click(within(table).getByRole("link", { name: new RegExp("^" + name) }));
      expect(navigate.mock.lastCall?.[0]).toBe(module);
      expect(navigate.mock.lastCall?.[1]?.toString()).toBe(query);
    }
    expect(within(table).getByText("Last applied: daily")).toBeVisible();
    expect(screen.queryByText(/files present/)).not.toBeInTheDocument();
  });
  it("reports a failure once, retries only that source and seeds the first successful topology", async () => {
    const user = userEvent.setup();
    const api = fakeApi();
    api.loadTopology.mockRejectedValueOnce(new Error("Read denied"));
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    expect(await screen.findAllByText("Tenant resources could not be inspected.")).toHaveLength(1);
    expect(screen.getByText("Read denied")).toBeInTheDocument();
    const retry = screen.getByRole("button", { name: "Retry resource inspection" });
    // The panel's one recovery action is the shared refresh control, not a text run.
    expect(retry).toHaveAttribute("data-refresh-button", "true");
    await user.click(retry);
    const table = await screen.findByRole("table", { name: "Tenant status" });
    expect(api.loadOverview).toHaveBeenCalledTimes(1);
    expect(within(table).getByText("default")).toBeVisible();
    api.loadTopology.mockRejectedValueOnce(new Error("Try again"));
    await user.click(screen.getByRole("button", { name: "Refresh Overview" }));
    expect(await screen.findByText("Tenant resources could not be inspected.")).toBeVisible();
    expect(screen.getByText("Try again")).toBeInTheDocument();
    expect(within(table).getByText("default")).toBeVisible();
    expect(
      screen.queryByText("No warnings or errors are currently reported."),
    ).not.toBeInTheDocument();
  });
  it("keeps Service failures separate from topology and exposes a scoped retry", async () => {
    const user = userEvent.setup();
    const api = fakeApi();
    api.loadOverview.mockRejectedValueOnce(new Error("Service read failed"));
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    await screen.findByRole("table", { name: "Tenant status" });
    expect(screen.getAllByText("Service status could not be read.")).toHaveLength(1);
    expect(screen.getByText("Service read failed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Build" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Retry Service status" }));
    expect(await screen.findByText("Running")).toBeVisible();
    expect(api.loadTopology).toHaveBeenCalledTimes(1);
  });
  it("shows failed catalog reads as unavailable rather than zero or healthy", async () => {
    const tenant = quietTopology.tenants[0];
    const api = fakeApi({
      tenants: [
        {
          ...tenant,
          components: { ...tenant.components, error: "Components unreadable" },
          agents: [
            {
              ...tenant.agents[0],
              named_configs: { count: 0, attention: [], error: "Configs unreadable" },
            },
          ],
        },
      ],
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    const table = await screen.findByRole("table", { name: "Tenant status" });
    expect(within(table).getAllByText("Catalog inspection failed")).toHaveLength(2);
    expect(within(table).queryByText("0 Configs")).not.toBeInTheDocument();
  });
  it("keeps Config attention as a direct specific-resource entry", async () => {
    const user = userEvent.setup();
    const navigate = vi.fn<ConsoleNavigate>();
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={navigate} onOperation={vi.fn()} />,
    );
    await screen.findByRole("table", { name: "Tenant status" });
    await user.click(within(attentionRegion()).getByRole("link", { name: /broken is incomplete/ }));
    expect(navigate.mock.lastCall?.[1]?.toString()).toBe(
      "tenant=managed%3Adefault&agent=codex&config=broken",
    );
  });
  it("puts full environment values in a keyboard disclosure without node paths", async () => {
    const user = userEvent.setup();
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />,
    );
    const table = await screen.findByRole("table", { name: "Tenant status" });
    const summary = screen.getByText("Environment details");
    act(() => summary.focus());
    await user.click(summary);
    expect(screen.getByText("/var/lib/aibox")).toBeVisible();
    expect(screen.getByText("1m 30s")).toBeVisible();
    expect(screen.getByText(/aibox:test · 0123456789ab/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Copy AIBox Root" }));
    expect(await navigator.clipboard.readText()).toBe("/var/lib/aibox");
    expect(within(table).queryByText("/var/lib/aibox/tenants/default")).not.toBeInTheDocument();
  });
  it("keeps build submission errors separate from Service health and allows retry", async () => {
    const user = userEvent.setup();
    const api = fakeApi(quietTopology);
    api.buildImage.mockRejectedValueOnce(new Error("Build rejected"));
    const onOperation = vi.fn();
    render(
      <OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={onOperation} />,
    );
    await screen.findByRole("table", { name: "Tenant status" });
    await user.click(screen.getByRole("button", { name: "Build" }));
    expect(await screen.findByText("The Runtime Image build failed.")).toBeVisible();
    expect(screen.getByText("Build rejected")).toBeInTheDocument();
    expect(screen.getByText("Running")).toBeVisible();
    expect(screen.getByRole("button", { name: "Build" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Build" }));
    await waitFor(() => expect(onOperation).toHaveBeenCalledWith(operation));
    expect(screen.queryByText("Build rejected")).not.toBeInTheDocument();
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

  it("leads with a cause and a next step, keeping the raw diagnostic behind a disclosure", async () => {
    const user = userEvent.setup();
    const api = fakeApi();
    const raw =
      "docker image inspect failed (exit status: 1): Cannot connect to the Docker daemon at " +
      "unix:///var/run/docker.sock. Is the docker daemon running?";
    api.loadOverview.mockResolvedValue({
      ...overview,
      docker: { status: "unavailable", error: raw },
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    const region = await screen.findByRole("region", { name: "Needs attention" });
    // The sentence names the cause and the fix; the errno never reaches it.
    expect(
      within(region).getByText("The Docker daemon is not running. Start Docker, then refresh."),
    ).toBeVisible();
    // The raw text is kept, but closed until asked for.
    const details = within(region).getByText(raw);
    expect(details).not.toBeVisible();
    await user.click(within(region).getByText("Technical details"));
    expect(details).toBeVisible();
  });

  it("keeps the pending attention copy when topology arrives before overview", async () => {
    const { api, resolveOverview, resolveTopology } = delayedApi();
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    resolveTopology();
    expect(await screen.findByRole("table", { name: "Tenant status" })).toBeInTheDocument();
    expectAttentionPending();
    resolveOverview();
    expect(
      await screen.findByText("No warnings or errors are currently reported."),
    ).toBeInTheDocument();
  });

  it("renders diagnostic-only attention as text, not a disabled action", async () => {
    const api = fakeApi();
    api.loadOverview.mockResolvedValue({
      ...overview,
      docker: { status: "unavailable", error: "Start Docker and refresh." },
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    await screen.findByRole("table", { name: "Tenant status" });
    expect(within(attentionRegion()).getByText("Docker is unavailable.")).toBeVisible();
    expect(within(attentionRegion()).getByText("Start Docker and refresh.")).toBeInTheDocument();
    expect(
      within(attentionRegion()).queryByRole("link", { name: /Docker/ }),
    ).not.toBeInTheDocument();
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
    await user.click(screen.getByRole("button", { name: "Refresh Overview" }));
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
    await user.click(screen.getByRole("button", { name: "Refresh Overview" }));
    expectAttentionPending();
    refresh.resolve(quietTopology);
    expect(
      await screen.findByText("No warnings or errors are currently reported."),
    ).toBeInTheDocument();
  });
});

/*
 * The attention panel was a horizontal `flex-wrap` run showing one row per
 * category with a `+N more` tail, and its severity rode on hue alone.
 */
describe("OverviewPage attention panel", () => {
  function crowdedTopology(): TopologyData {
    return {
      tenants: [
        {
          kind: "host",
          name: null,
          display_name: "Host Tenant",
          home: "/home/test",
          exists: true,
          agents: [
            {
              agent: "codex",
              current_config: { present_files: 2, expected_files: 2 },
              named_configs: { count: 0, attention: [] },
              sessions: { count: 0 },
              application: { last_application: null, drift: "dirty" },
            },
            {
              agent: "claude",
              current_config: { present_files: 1, expected_files: 1 },
              named_configs: {
                count: 3,
                attention: [
                  { name: "one", state: "incomplete" },
                  { name: "two", state: "incomplete" },
                  { name: "three", state: "invalid" },
                ],
              },
              sessions: { count: 0 },
              application: { last_application: null, drift: "dirty" },
            },
          ],
          components: {
            total: 4,
            installed: 2,
            attention: [
              {
                kind: "python",
                supports_version: true,
                status: "modified",
                version: null,
                error: null,
              },
              {
                kind: "node",
                supports_version: true,
                status: "incomplete",
                version: null,
                error: null,
              },
            ],
          },
        },
      ],
    } satisfies TopologyData;
  }

  function rowTexts() {
    return within(attentionRegion())
      .getAllByRole("listitem")
      .map((row) => row.textContent ?? "");
  }

  it("lists one row per condition instead of folding the rest into +N more", async () => {
    render(
      <OverviewPage
        api={fakeApi(crowdedTopology())}
        operation={null}
        onNavigate={vi.fn()}
        onOperation={vi.fn()}
      />,
    );
    await screen.findByRole("table", { name: "Tenant status" });
    await waitFor(() => expect(rowTexts()).toHaveLength(6));
    expect(rowTexts().join(" ")).not.toContain("more");
    // Every named Config that needs work is reachable, not just the first.
    await userEvent
      .setup()
      .click(within(attentionRegion()).getByRole("button", { name: "Show 1 more" }));
    const all = rowTexts();
    expect(all).toHaveLength(7);
    for (const name of ["one", "two", "three"])
      expect(all.some((row) => row.includes(name))).toBe(true);
  });

  it("states the total even while the list is capped", async () => {
    render(
      <OverviewPage
        api={fakeApi(crowdedTopology())}
        operation={null}
        onNavigate={vi.fn()}
        onOperation={vi.fn()}
      />,
    );
    expect(await screen.findByText("Needs attention · 7")).toBeVisible();
    expect(within(attentionRegion()).getAllByRole("listitem")).toHaveLength(6);
  });

  it("puts errors above warnings rather than following the order sources are read", async () => {
    render(
      <OverviewPage
        api={fakeApi(crowdedTopology())}
        operation={null}
        onNavigate={vi.fn()}
        onOperation={vi.fn()}
      />,
    );
    await screen.findByRole("table", { name: "Tenant status" });
    await waitFor(() => expect(rowTexts()).toHaveLength(6));
    // The one error in this topology is an invalid Named Config.
    expect(rowTexts()[0]).toContain("three is invalid");
  });

  /*
   * Simulated for deuteranopia the two tones land ~6 ΔE apart in the light
   * theme, so a shared glyph made an error and a warning the same row.
   */
  it("gives an error and a warning different marks, not only different colours", async () => {
    render(
      <OverviewPage
        api={fakeApi(crowdedTopology())}
        operation={null}
        onNavigate={vi.fn()}
        onOperation={vi.fn()}
      />,
    );
    await screen.findByRole("table", { name: "Tenant status" });
    await waitFor(() => expect(rowTexts()).toHaveLength(6));
    const marks = within(attentionRegion())
      .getAllByRole("listitem")
      .map((row) => row.querySelector("svg")?.getAttribute("class") ?? "");
    const errorMark = marks[0];
    const warningMark = marks[marks.length - 1];
    expect(errorMark).not.toBe("");
    expect(warningMark).not.toBe("");
    expect(errorMark).not.toBe(warningMark);
  });

  it("hides the cap control once every condition fits", async () => {
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />,
    );
    await screen.findByRole("table", { name: "Tenant status" });
    expect(
      within(attentionRegion()).queryByRole("button", { name: /Show \d+ more/ }),
    ).not.toBeInTheDocument();
  });
});

describe("OverviewPage Tenant table", () => {
  /*
   * A third of this table's tab stops used to land where a sibling in the same
   * cell already landed: the Component count repeated "Manage components", and
   * a drift warning repeated the Current Config link beside it.
   */
  it("gives each cell one link per destination and states the rest as facts", async () => {
    render(
      <OverviewPage api={fakeApi()} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />,
    );
    const table = await screen.findByRole("table", { name: "Tenant status" });
    for (const cell of table.querySelectorAll("tbody th, tbody td")) {
      const targets = [...cell.querySelectorAll("a")].map((a) => a.getAttribute("href"));
      expect(new Set(targets).size).toBe(targets.length);
    }
  });

  it("keeps a drift warning readable without making it a second link", async () => {
    const tenant = quietTopology.tenants[0];
    const api = fakeApi({
      tenants: [
        {
          ...tenant,
          agents: [
            {
              ...tenant.agents[0],
              application: { last_application: null, drift: "dirty" },
            },
          ],
        },
      ],
    });
    render(<OverviewPage api={api} operation={null} onNavigate={vi.fn()} onOperation={vi.fn()} />);
    const table = await screen.findByRole("table", { name: "Tenant status" });
    expect(within(table).getByText(/Current Config: Differs/)).toBeVisible();
    expect(
      within(table).queryByRole("link", { name: /Current Config: Differs/ }),
    ).not.toBeInTheDocument();
  });
});
