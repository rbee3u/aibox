import { describe, expect, it } from "vitest";
import {
  clampZoom,
  buildTopologyTree,
  componentNode,
  defaultExpansion,
  firstComponentAttentionTarget,
  layoutTopology,
  structuralIds,
  visibleTopology,
  type TopologyNode,
} from "@/features/overview/topology/topologyModel";
import type { TopologyData } from "@/api/overview";

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

  it("only opens the protected default Tenant, falling back to Host", () => {
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

  it("navigates Component nodes and attention to the Tenant only", () => {
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
    expect(node.detail).toBe("0/1 installed · 1 needs attention");
    expect(node.children[0].target?.query?.toString()).toBe("tenant=managed%3Adefault");
    expect(node.children[0].target?.query?.has("component")).toBe(false);

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
    expect(target.query?.toString()).toBe("tenant=managed%3Adefault");
    expect(target.query?.has("component")).toBe(false);
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
    const current = agent.children.find((node) => node.label === "Current Config");
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
});
