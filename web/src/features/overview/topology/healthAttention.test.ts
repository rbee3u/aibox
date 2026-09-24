import { describe, expect, it } from "vitest";
import {
  attentionPanelKind,
  bySeverity,
  componentAttentions,
  configAttentions,
  sessionAttentions,
  topologyAttentions,
} from "@/features/overview/topology/healthAttention";
import type { AttentionItem } from "@/features/overview/resourceTree";
import type { TopologyData } from "@/api/overview";

type Tenant = TopologyData["tenants"][number];
type ManagedTenant = Extract<Tenant, { kind: "managed" }>;
type Agent = Tenant["agents"][number];

function managed(overrides: Partial<Omit<ManagedTenant, "kind">> = {}): ManagedTenant {
  return {
    kind: "managed",
    name: "default",
    display_name: "default",
    home: "/tmp/default",
    exists: true,
    agents: [],
    components: { total: 0, installed: 0, attention: [] },
    ...overrides,
  };
}
function agent(overrides: Partial<Agent> = {}): Agent {
  return {
    agent: "codex",
    current_config: { present_files: 2, expected_files: 2 },
    named_configs: { count: 0, attention: [] },
    sessions: { count: 0 },
    application: { last_application: null, drift: "clean" },
    ...overrides,
  };
}
function details(items: AttentionItem[]): string[] {
  return items.map((item) => item.detail);
}

describe("attentionPanelKind", () => {
  it("never claims healthy until both sources have settled", () => {
    expect(
      attentionPanelKind({ itemCount: 0, overviewSettled: false, topologySettled: false }),
    ).toBe("pending");
    expect(
      attentionPanelKind({ itemCount: 0, overviewSettled: true, topologySettled: false }),
    ).toBe("pending");
    expect(
      attentionPanelKind({ itemCount: 0, overviewSettled: false, topologySettled: true }),
    ).toBe("pending");
  });

  it("shows known items immediately, even while the other source is still loading", () => {
    expect(
      attentionPanelKind({ itemCount: 1, overviewSettled: true, topologySettled: false }),
    ).toBe("items");
    expect(
      attentionPanelKind({ itemCount: 2, overviewSettled: false, topologySettled: false }),
    ).toBe("items");
  });

  it("uses the healthy empty copy only after both sources settle with no items", () => {
    expect(attentionPanelKind({ itemCount: 0, overviewSettled: true, topologySettled: true })).toBe(
      "healthy",
    );
    expect(attentionPanelKind({ itemCount: 1, overviewSettled: true, topologySettled: true })).toBe(
      "items",
    );
  });
});

describe("configAttentions", () => {
  it("names each Current Config reason with its own tone", () => {
    for (const [drift, detail, tone] of [
      ["dirty", "Current Config differs", "warning"],
      ["source-missing", "Current Config source is missing", "warning"],
      ["comparison-error", "Current Config comparison failed", "error"],
    ] as const) {
      const data = {
        tenants: [managed({ agents: [agent({ application: { last_application: null, drift } })] })],
      } satisfies TopologyData;
      const [item] = configAttentions(data);
      expect(item.detail).toBe(`default · Codex · ${detail}`);
      expect(item.tone).toBe(tone);
      expect(item.target?.query?.toString()).toBe("tenant=managed%3Adefault&agent=codex&current=1");
    }
  });

  it("keeps a Current Config inspection failure and its Drift as separate conditions", () => {
    const data = {
      tenants: [
        managed({
          agents: [
            agent({
              current_config: { present_files: 1, expected_files: 2, error: "unreadable" },
              application: { last_application: null, drift: "dirty" },
            }),
          ],
        }),
      ],
    } satisfies TopologyData;
    expect(details(configAttentions(data))).toEqual([
      "default · Codex · Current Config inspection failed",
      "default · Codex · Current Config differs",
    ]);
    expect(configAttentions(data).map((item) => item.tone)).toEqual(["error", "warning"]);
  });

  it("opens the Named Configs catalog when catalog inspection failed", () => {
    const data = {
      tenants: [
        managed({
          agents: [agent({ named_configs: { count: 0, attention: [], error: "unreadable" } })],
        }),
      ],
    } satisfies TopologyData;
    const [item] = configAttentions(data);
    expect(item.detail).toBe("default · Codex · Named Configs inspection failed");
    expect(item.tone).toBe("error");
    expect(item.target?.query?.toString()).toBe("tenant=managed%3Adefault&agent=codex&named=1");
  });
  it("reports a Home that could not be walked, and stays quiet about an empty one", () => {
    const unreadable = {
      tenants: [managed({ agents: [agent({ sessions: { count: 0, error: "unreadable" } })] })],
    } satisfies TopologyData;
    const [item] = sessionAttentions(unreadable);
    expect(item.detail).toBe("default · Codex · Session inspection failed");
    expect(item.tone).toBe("error");
    expect(item.target?.module).toBe("sessions");
    expect(item.target?.query?.toString()).toBe("tenant=managed%3Adefault&agent=codex");
    const empty = {
      tenants: [managed({ agents: [agent({ sessions: { count: 0 } })] })],
    } satisfies TopologyData;
    expect(sessionAttentions(empty)).toEqual([]);
  });

  /*
   * The panel used to name the first hit and append `+N more` to its sentence,
   * so a second broken Config was a number the reader could not act on.
   */
  it("lists every Named Config that needs work, each targeting its own Config", () => {
    const data = {
      tenants: [
        managed({
          agents: [
            agent({
              named_configs: {
                count: 3,
                attention: [
                  { name: "broken", state: "incomplete" },
                  { name: "bad", state: "invalid" },
                  { name: "fine", state: "ready" },
                ],
              },
            }),
          ],
        }),
      ],
    } satisfies TopologyData;
    const items = configAttentions(data);
    expect(details(items)).toEqual([
      "default · Codex · broken is incomplete",
      "default · Codex · bad is invalid",
    ]);
    expect(items.map((item) => item.tone)).toEqual(["warning", "error"]);
    expect(items[1].target?.query?.toString()).toBe(
      "tenant=managed%3Adefault&agent=codex&config=bad",
    );
  });

  it("reports every Tenant and Agent rather than stopping at the first", () => {
    const data = {
      tenants: [
        {
          kind: "host",
          name: null,
          display_name: "Host Tenant",
          home: "/home/test",
          exists: true,
          agents: [
            agent({ application: { last_application: null, drift: "dirty" } }),
            agent({ agent: "claude", application: { last_application: null, drift: "dirty" } }),
          ],
          components: { total: 0, installed: 0, attention: [] },
        },
        managed({ agents: [agent({ application: { last_application: null, drift: "dirty" } })] }),
      ],
    } satisfies TopologyData;
    expect(details(configAttentions(data))).toEqual([
      "Host Tenant · Codex · Current Config differs",
      "Host Tenant · Claude · Current Config differs",
      "default · Codex · Current Config differs",
    ]);
  });

  it("gives every item a distinct key so one category can hold many rows", () => {
    const data = {
      tenants: [
        managed({
          agents: [
            agent({
              application: { last_application: null, drift: "dirty" },
              named_configs: {
                count: 2,
                attention: [
                  { name: "one", state: "incomplete" },
                  { name: "two", state: "incomplete" },
                ],
              },
            }),
          ],
        }),
      ],
    } satisfies TopologyData;
    const keys = configAttentions(data).map((item) => item.key);
    expect(new Set(keys).size).toBe(keys.length);
  });
});

describe("componentAttentions", () => {
  it("names each Component that needs work and targets it", () => {
    const data = {
      tenants: [
        managed({
          components: {
            total: 3,
            installed: 1,
            attention: [
              {
                kind: "python",
                supports_version: true,
                status: "modified",
                version: "3.14.7",
                error: null,
              },
              {
                kind: "claude-statusline",
                supports_version: false,
                status: "incomplete",
                version: null,
                error: null,
              },
              {
                kind: "node",
                supports_version: true,
                status: "installed",
                version: "24",
                error: null,
              },
            ],
          },
        }),
      ],
    } satisfies TopologyData;
    const items = componentAttentions(data);
    expect(details(items)).toEqual([
      "default · Python differs from the AIBox definition",
      "default · Claude Statusline is incomplete",
    ]);
    expect(items[0].target?.query?.toString()).toBe("tenant=managed%3Adefault&component=python");
  });

  it("opens the Tenant when the Component catalog itself failed", () => {
    const data = {
      tenants: [
        managed({ components: { total: 0, installed: 0, attention: [], error: "denied" } }),
      ],
    } satisfies TopologyData;
    const [item] = componentAttentions(data);
    expect(item.detail).toBe("default · Components inspection failed");
    expect(item.tone).toBe("error");
    expect(item.target?.query?.has("component")).toBe(false);
  });
});

describe("bySeverity", () => {
  /*
   * Collection order is the order the sources are read, which put the Runtime
   * Image and the Host Tenant above a failed Config comparison.
   */
  it("lifts errors above warnings and keeps collection order inside each tone", () => {
    const items: AttentionItem[] = [
      { key: "a", label: "Runtime Image", detail: "missing", tone: "warning" },
      { key: "b", label: "Host Tenant", detail: "unavailable", tone: "warning" },
      { key: "c", label: "Configs", detail: "comparison failed", tone: "error" },
      { key: "d", label: "Components", detail: "denied", tone: "error" },
    ];
    expect(bySeverity(items).map((item) => item.key)).toEqual(["c", "d", "a", "b"]);
  });

  it("leaves the input array untouched", () => {
    const items: AttentionItem[] = [
      { key: "a", label: "A", detail: "warning", tone: "warning" },
      { key: "b", label: "B", detail: "error", tone: "error" },
    ];
    bySeverity(items);
    expect(items.map((item) => item.key)).toEqual(["a", "b"]);
  });
});

describe("topologyAttentions", () => {
  it("counts one row per real condition so a capped list can still state the total", () => {
    const data = {
      tenants: [
        managed({
          agents: [
            agent({
              application: { last_application: null, drift: "dirty" },
              named_configs: { count: 1, attention: [{ name: "broken", state: "incomplete" }] },
            }),
          ],
          components: {
            total: 2,
            installed: 1,
            attention: [
              {
                kind: "python",
                supports_version: true,
                status: "modified",
                version: null,
                error: null,
              },
            ],
          },
        }),
      ],
    } satisfies TopologyData;
    expect(topologyAttentions(data)).toHaveLength(3);
  });
});
