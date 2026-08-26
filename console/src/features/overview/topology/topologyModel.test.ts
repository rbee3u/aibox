import { describe, expect, it } from "vitest";
import {
  clampZoom,
  componentNode,
  filterByAttention,
  firstComponentAttentionTarget,
  layoutTopology,
  searchTopology,
  visibleTopology,
  type TopologyNode,
} from "@/features/overview/topology/topologyModel";
import type { TopologyData } from "@/api/overview";

const tree: TopologyNode = {
  id: "service",
  parentId: null,
  label: "aibox Service",
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
          detail: "1/2 installed",
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

  it.each([
    ["host", ["tenant:host"], ["service"]],
    ["installed", ["tenant:managed:default/components"], ["service", "tenant:managed:default"]],
    ["missing", [], []],
  ])("searches %s and retains its ancestor context", (query, matches, context) => {
    const result = searchTopology(tree, query);
    expect([...result.matches]).toEqual(matches);
    expect([...result.context]).toEqual(context);
    expect(result.firstMatch).toBe(matches[0] ?? null);
  });

  it("filters healthy leaves while retaining attention ancestors", () => {
    const filtered = filterByAttention(tree);
    expect(filtered?.children.map((node) => node.id)).toEqual(["tenant:managed:default"]);
    expect(filtered?.children[0].children.map((node) => node.id)).toEqual([
      "tenant:managed:default/components",
    ]);
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
    const node = componentNode("tenant:managed:default", { kind: "managed", name: "default" }, [
      {
        kind: "python",
        supports_version: true,
        status: "modified",
        version: "3.14.7",
        error: null,
      },
    ]);
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
            entries: [
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
});
