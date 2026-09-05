import { describe, expect, it } from "vitest";
import {
  agentCardDetail,
  agentCardTooltip,
  clampZoom,
  buildTopologyTree,
  componentNode,
  defaultExpansion,
  firstComponentAttentionTarget,
  layoutTopology,
  inspectorFacts,
  sessionFacts,
  sessionLoadDetail,
  sessionLoadTooltip,
  structuralIds,
  topologyNodeDisclosure,
  topologyNodeSize,
  visibleTopology,
  type TopologyNode,
} from "@/features/overview/topology/topologyModel";
import type { TopologyAgent, TopologyData } from "@/api/overview";

const tree: TopologyNode = {
  id: "service",
  parentId: null,
  label: "AIBox Service",
  icon: "service",
  tone: "warning",
  children: [
    {
      id: "tenant:host",
      parentId: "service",
      label: "Host Tenant",
      detail: "/Users/example",
      icon: "host",
      tone: "good",
      children: [],
    },
    {
      id: "tenant:managed:default",
      parentId: "service",
      label: "default",
      icon: "tenant",
      tone: "warning",
      children: [
        {
          id: "tenant:managed:default/components",
          parentId: "tenant:managed:default",
          label: "Components",
          detail: "1/2 installed · 1 needs attention",
          icon: "components",
          tone: "warning",
          children: [],
        },
      ],
    },
  ],
};

describe("overview topology algorithms", () => {
  it.each([
    [0.2, 0.65],
    [0.94, 0.9],
    [1.06, 1.1],
    [2, 1.5],
  ])("clamps and rounds zoom %s to %s", (input, expected) => {
    expect(clampZoom(input)).toBe(expected);
  });

  it("only opens the protected default Tenant when nothing needs attention, falling back to Host", () => {
    const data = {
      tenants: [
        {
          kind: "managed",
          name: "studio",
          display_name: "studio",
          home: "/tmp/studio",
          exists: true,
          agents: [],
          components: { total: 0, installed: 0, attention: [] },
        },
        {
          kind: "managed",
          name: "default",
          display_name: "default",
          home: "/tmp/default",
          exists: true,
          agents: [],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    } satisfies TopologyData;
    expect([...defaultExpansion(data)]).toEqual(["tenant:managed:default"]);
    expect([
      ...defaultExpansion({
        tenants: [
          {
            kind: "host",
            name: null,
            display_name: "Host Tenant",
            home: "/Users/example",
            exists: true,
            agents: [],
            components: { total: 0, installed: 0, attention: [] },
          },
        ],
      }),
    ]).toEqual(["tenant:host"]);
  });

  it("sizes Component and Named Config leaves as wide as Tenant cards", () => {
    expect(topologyNodeSize("component")).toEqual({ width: 184, height: 38, kind: "leaf" });
    expect(topologyNodeSize("config")).toEqual({ width: 184, height: 38, kind: "leaf" });
    expect(topologyNodeSize("tenant").width).toBe(184);
  });

  it("lays out only expanded branches with stable parent-child edges", () => {
    const visible = visibleTopology(tree, new Set(["tenant:managed:default"]), new Set<string>());
    const layout = layoutTopology(visible, 900);

    expect(layout.nodes.map((entry) => entry.node.id)).toEqual([
      "service",
      "tenant:host",
      "tenant:managed:default",
      "tenant:managed:default/components",
    ]);
    expect(layout.edges.map((edge) => [edge.parentId, edge.childId])).toEqual([
      ["service", "tenant:host"],
      ["service", "tenant:managed:default"],
      ["tenant:managed:default", "tenant:managed:default/components"],
    ]);
  });

  it("navigates Component leaf nodes with a Component query", () => {
    const node = componentNode(
      "tenant:managed:default",
      { kind: "managed", name: "default" },
      {
        total: 1,
        installed: 0,
        attention: [
          {
            kind: "python",
            supports_version: true,
            status: "modified",
            version: "3.14.7",
            error: null,
          },
        ],
      },
    );
    expect(node.detail).toBe("0/1 installed");
    expect(node.children[0].target?.query?.toString()).toBe(
      "tenant=managed%3Adefault&component=python",
    );
    expect(node.target?.query?.toString()).toBe("tenant=managed%3Adefault");
    expect(node.target?.query?.has("component")).toBe(false);

    const data = {
      tenants: [
        {
          kind: "managed",
          name: "default",
          display_name: "default",
          home: "/tmp/default",
          exists: true,
          agents: [],
          components: {
            total: 1,
            installed: 0,
            attention: [
              {
                kind: "python",
                supports_version: true,
                status: "modified",
                version: "3.14.7",
                error: null,
              },
            ],
          },
        },
      ],
    } satisfies TopologyData;
    const target = firstComponentAttentionTarget(data);
    expect(target.module).toBe("tenants");
    expect(target.query?.toString()).toBe("tenant=managed%3Adefault&component=python");
    expect(target.query?.has("component")).toBe(true);
  });

  it("keeps Sessions as terminal metrics and expands structural containers only", () => {
    const data = {
      tenants: [
        {
          kind: "managed",
          name: "default",
          display_name: "default",
          home: "/tmp/default",
          exists: true,
          agents: [
            {
              agent: "codex",
              current_config: { present_files: 1, expected_files: 1 },
              named_configs: { count: 2, attention: [] },
              application: { last_application: null, drift: "clean" },
            },
          ],
          components: { total: 2, installed: 1, attention: [] },
        },
      ],
    } satisfies TopologyData;
    const built = buildTopologyTree(data, {}, null);
    const agent = built.children[0].children[0];
    const sessions = agent.children.find((node) => node.label === "Sessions");
    expect(sessions?.children).toEqual([]);
    const named = agent.children.find((node) => node.label === "Named Configs");
    expect(named?.target?.query?.toString()).toBe("tenant=managed%3Adefault&agent=codex&named=1");
    expect(named?.facts).toEqual([{ label: "Attention", value: "None need attention" }]);
    const current = agent.children.find((node) => node.label === "Current Config");
    expect(current).toMatchObject({
      tone: "neutral",
      detail: "1/1 files present",
    });
    expect(current?.facts).toEqual([]);
    expect(current?.target?.query?.toString()).toBe(
      "tenant=managed%3Adefault&agent=codex&current=1",
    );
    expect([...structuralIds(data)]).toEqual([
      "tenant:managed:default",
      "tenant:managed:default/agent:codex",
      "tenant:managed:default/agent:codex/named-configs",
      "tenant:managed:default/components",
    ]);
  });

  it("marks Current Config leaves with Application Drift the same way Attention names them", () => {
    const currentOf = (
      drift: TopologyAgent["application"]["drift"],
      extra?: { error?: string; detail?: string },
    ) => {
      const built = buildTopologyTree(
        {
          tenants: [
            {
              kind: "managed",
              name: "default",
              display_name: "default",
              home: "/tmp/default",
              exists: true,
              agents: [
                {
                  agent: "codex",
                  current_config: {
                    present_files: 2,
                    expected_files: 2,
                    error: extra?.error,
                  },
                  named_configs: { count: 0, attention: [] },
                  application: {
                    last_application: { applied: "openai", applied_at: "2026-08-18T02:00:00Z" },
                    drift,
                    detail: extra?.detail,
                  },
                },
              ],
              components: { total: 0, installed: 0, attention: [] },
            },
          ],
        },
        {},
        null,
      );
      return built.children[0].children[0].children.find((node) => node.label === "Current Config");
    };

    expect(currentOf("dirty", { detail: "differs from last applied" })).toMatchObject({
      tone: "warning",
      detail: "Dirty",
      title: "differs from last applied",
      facts: [
        { label: "Last applied", value: "openai · 2026-08-18 10:00:00" },
        { label: "Files", value: "2/2 present" },
      ],
    });
    expect(currentOf("source-missing")).toMatchObject({
      tone: "warning",
      detail: "Source Missing",
      facts: [
        { label: "Last applied", value: "openai · 2026-08-18 10:00:00" },
        { label: "Files", value: "2/2 present" },
      ],
    });
    expect(currentOf("comparison-error")).toMatchObject({
      tone: "error",
      detail: "Comparison Error",
      facts: [
        { label: "Last applied", value: "openai · 2026-08-18 10:00:00" },
        { label: "Files", value: "2/2 present" },
      ],
    });
    expect(currentOf("dirty", { error: "unreadable" })).toMatchObject({
      tone: "error",
      detail: "Inspection failed",
      title: "unreadable",
      facts: [{ label: "Last applied", value: "openai · 2026-08-18 10:00:00" }],
    });
    expect(currentOf("clean")).toMatchObject({
      tone: "neutral",
      detail: "2/2 files present",
      facts: [{ label: "Last applied", value: "openai · 2026-08-18 10:00:00" }],
    });
  });

  it("abbreviates Tenant homes when the Host Home is known", () => {
    const data = {
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
    } satisfies TopologyData;
    const built = buildTopologyTree(data, {}, "/home/test");
    const host = built.children.find((node) => node.id === "tenant:host");
    const managed = built.children.find((node) => node.id === "tenant:managed:default");
    expect(host).toMatchObject({
      detail: "~",
      tooltip: "/home/test",
      facts: [{ label: "Home", value: "/home/test" }],
    });
    expect(host?.title).toBeUndefined();
    expect(managed).toMatchObject({
      detail: "~/.aibox/tenants/default",
      tooltip: "/home/test/.aibox/tenants/default",
    });
    expect(managed?.title).toBeUndefined();
    const unresolved = buildTopologyTree(data, {}, null).children[0];
    expect(unresolved).toMatchObject({
      detail: "/home/test",
      tooltip: "/home/test",
    });
    expect(unresolved.title).toBeUndefined();
  });

  it("shortens Agent and Session card copy and discloses the full phrase", () => {
    const dirty = {
      agent: "codex",
      current_config: { present_files: 2, expected_files: 2 },
      named_configs: { count: 0, attention: [] },
      application: {
        last_application: { applied: "openai", applied_at: "2026-08-18T02:00:00Z" },
        drift: "dirty",
      },
    } satisfies TopologyAgent;
    expect(agentCardDetail(dirty)).toBe("openai · Dirty");
    expect(agentCardTooltip(dirty)).toBe("Last applied openai · Dirty");
    expect(
      agentCardDetail({ ...dirty, application: { last_application: null, drift: "untracked" } }),
    ).toBe("Untracked");
    expect(
      agentCardTooltip({ ...dirty, application: { last_application: null, drift: "untracked" } }),
    ).toBe("Config Drift Untracked");
    expect(sessionLoadDetail()).toBe("Load count");
    expect(sessionFacts()).toEqual([{ label: "Summary", value: "Load on demand" }]);
    expect(
      sessionFacts({ state: "loaded", data: { count: 11, warnings: [], partial: false } }),
    ).toEqual([{ label: "Summary", value: "11 Sessions" }]);
    expect(sessionLoadTooltip()).toBe("Load count on demand");
    expect(sessionLoadDetail({ state: "loading" })).toBe("Discovering");
    expect(sessionLoadTooltip({ state: "loading" })).toBe("Discovering Transcripts");
    expect(
      topologyNodeDisclosure({
        id: "tenant:host",
        parentId: "service",
        label: "Host Tenant",
        detail: "~",
        tooltip: "/home/test",
        icon: "host",
        tone: "good",
        children: [],
      }),
    ).toBe("/home/test");
    expect(
      topologyNodeDisclosure({
        id: "agent",
        parentId: "tenant",
        label: "Codex",
        detail: "openai · Dirty",
        tooltip: "Last applied openai · Dirty",
        title: "differs from last applied",
        icon: "codex",
        tone: "warning",
        children: [],
      }),
    ).toBe("Last applied openai · Dirty\ndiffers from last applied");
    expect(
      topologyNodeDisclosure({
        id: "sessions",
        parentId: "agent",
        label: "Sessions",
        detail: "3 Sessions",
        icon: "sessions",
        tone: "neutral",
        children: [],
      }),
    ).toBeUndefined();
    expect(
      inspectorFacts({
        id: "component",
        parentId: "components",
        label: "Claude Statusline",
        detail: "Modified",
        icon: "component",
        tone: "warning",
        children: [],
      }),
    ).toEqual([{ label: "Status", value: "Modified" }]);
    expect(
      inspectorFacts({
        id: "current",
        parentId: "agent",
        label: "Current Config",
        detail: "Dirty",
        facts: [
          { label: "Last applied", value: "openai · 2026-08-18 10:00:00" },
          { label: "Files", value: "2/2 present" },
        ],
        icon: "current",
        tone: "warning",
        children: [],
      }),
    ).toEqual([
      { label: "Last applied", value: "openai · 2026-08-18 10:00:00" },
      { label: "Files", value: "2/2 present" },
    ]);
  });
});
