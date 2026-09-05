import { describe, expect, it } from "vitest";
import {
  attentionPanelKind,
  attentionTargetDetail,
  attentionValue,
  componentAttentionItem,
  configAttentionItem,
  firstComponentAttention,
  firstConfigAttention,
  firstConfigAttentionTarget,
  defaultExpansion,
  summarizeTopology,
} from "@/features/overview/topology/healthAttention";
import { attentionCountLabel } from "@/features/overview/topology/coreTree";
import type { TopologyData } from "@/api/overview";

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

describe("attention copy", () => {
  it("uses singular needs for one item and plural need otherwise", () => {
    expect(attentionValue(0)).toBe("Healthy");
    expect(attentionValue(1)).toBe("1 needs attention");
    expect(attentionValue(2)).toBe("2 need attention");
    expect(attentionCountLabel(0)).toBe("0 need attention");
    expect(attentionCountLabel(1)).toBe("1 needs attention");
  });
});

describe("firstConfigAttentionTarget", () => {
  it("opens the Named Configs catalog when catalog inspection failed", () => {
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
              current_config: { present_files: 2, expected_files: 2 },
              named_configs: { count: 0, attention: [], error: "catalog unreadable" },
              application: { last_application: null, drift: "clean" },
            },
          ],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    } satisfies TopologyData;
    const target = firstConfigAttentionTarget(data);
    expect(target.module).toBe("configs");
    expect(target.query?.toString()).toBe("tenant=managed%3Adefault&agent=codex&named=1");
    expect(firstConfigAttention(data).subject).toBe(
      "default · Codex · Named Configs inspection failed",
    );
  });

  it("names the first incomplete Named Config", () => {
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
              current_config: { present_files: 2, expected_files: 2 },
              named_configs: {
                count: 2,
                attention: [{ name: "broken", state: "incomplete" }],
              },
              application: { last_application: null, drift: "clean" },
            },
          ],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    } satisfies TopologyData;
    const found = firstConfigAttention(data);
    expect(found.subject).toBe("default · Codex · broken is incomplete");
    expect(found.target.query?.toString()).toBe(
      "tenant=managed%3Adefault&agent=codex&config=broken",
    );
    expect(configAttentionItem(data, summarizeTopology(data)).detail).toBe(
      "default · Codex · broken is incomplete",
    );
  });

  it("names Current Config when Drift needs work", () => {
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
              current_config: { present_files: 2, expected_files: 2 },
              named_configs: { count: 1, attention: [] },
              application: { last_application: null, drift: "dirty" },
            },
          ],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    } satisfies TopologyData;
    expect(firstConfigAttention(data).subject).toBe("default · Codex · Current Config is dirty");
  });

  it("names Current Config source missing and comparison failure", () => {
    const missing = {
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
              current_config: { present_files: 2, expected_files: 2 },
              named_configs: { count: 0, attention: [] },
              application: { last_application: null, drift: "source-missing" },
            },
          ],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    } satisfies TopologyData;
    const failed = {
      tenants: [
        {
          kind: "host",
          name: null,
          display_name: "Host Tenant",
          home: "/home/test",
          exists: true,
          agents: [
            {
              agent: "claude",
              current_config: { present_files: 1, expected_files: 1 },
              named_configs: { count: 0, attention: [] },
              application: { last_application: null, drift: "comparison-error" },
            },
          ],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    } satisfies TopologyData;
    expect(firstConfigAttention(missing).subject).toBe(
      "default · Codex · Current Config source is missing",
    );
    expect(firstConfigAttention(failed).subject).toBe(
      "Host Tenant · Claude · Current Config comparison failed",
    );
  });

  it("names Current Config inspection failure before Drift", () => {
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
              current_config: { present_files: 1, expected_files: 2, error: "unreadable" },
              named_configs: { count: 0, attention: [] },
              application: { last_application: null, drift: "dirty" },
            },
          ],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    } satisfies TopologyData;
    expect(firstConfigAttention(data).subject).toBe(
      "default · Codex · Current Config inspection failed",
    );
  });
});

describe("firstComponentAttention", () => {
  it("names the first Component that needs work", () => {
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
            total: 2,
            installed: 1,
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
    const found = firstComponentAttention(data);
    expect(found.subject).toBe("default · Python is modified");
    expect(found.target.query?.toString()).toBe("tenant=managed%3Adefault&component=python");
    expect(componentAttentionItem(data, summarizeTopology(data)).detail).toBe(
      "default · Python is modified",
    );
  });

  it("opens the Tenant only when the Component catalog itself failed", () => {
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
            total: 0,
            installed: 0,
            attention: [],
            error: "permission denied",
          },
        },
      ],
    } satisfies TopologyData;
    const found = firstComponentAttention(data);
    expect(found.subject).toBe("default · Components inspection failed");
    expect(found.target.query?.toString()).toBe("tenant=managed%3Adefault");
    expect(found.target.query?.has("component")).toBe(false);
  });
});

describe("defaultExpansion", () => {
  it("opens the first Config attention Agent instead of every default Agent", () => {
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
              current_config: { present_files: 2, expected_files: 2 },
              named_configs: {
                count: 1,
                attention: [{ name: "broken", state: "incomplete" }],
              },
              application: { last_application: null, drift: "clean" },
            },
            {
              agent: "claude",
              current_config: { present_files: 1, expected_files: 1 },
              named_configs: { count: 0, attention: [] },
              application: { last_application: null, drift: "clean" },
            },
          ],
          components: { total: 0, installed: 0, attention: [] },
        },
      ],
    } satisfies TopologyData;
    expect([...defaultExpansion(data)]).toEqual([
      "tenant:managed:default",
      "tenant:managed:default/agent:codex",
    ]);
  });

  it("opens the first Component attention Tenant and its Components group", () => {
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
          home: "/tmp/shadow1",
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
    expect([...defaultExpansion(data)]).toEqual([
      "tenant:managed:shadow1",
      "tenant:managed:shadow1/components",
    ]);
  });

  it("prefers Config attention over Component attention", () => {
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
              current_config: { present_files: 2, expected_files: 2 },
              named_configs: { count: 0, attention: [] },
              application: { last_application: null, drift: "dirty" },
            },
          ],
          components: {
            total: 1,
            installed: 0,
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
        },
      ],
    } satisfies TopologyData;
    expect([...defaultExpansion(data)]).toEqual([
      "tenant:managed:default",
      "tenant:managed:default/agent:codex",
    ]);
  });
});

describe("attentionTargetDetail", () => {
  it("keeps a single subject and adds a remainder for more items", () => {
    expect(attentionTargetDetail("default · Codex · broken", 1)).toBe("default · Codex · broken");
    expect(attentionTargetDetail("default · Codex · broken", 3)).toBe(
      "default · Codex · broken · +2 more",
    );
  });
});
