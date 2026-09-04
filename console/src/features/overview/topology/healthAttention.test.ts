import { describe, expect, it } from "vitest";
import {
  attentionPanelKind,
  attentionValue,
  firstConfigAttentionTarget,
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
  });
});
