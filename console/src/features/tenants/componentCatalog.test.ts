import { describe, expect, it } from "vitest";
import type { ComponentLatestSnapshot, ComponentRow } from "@/api/tenants";
import {
  abbreviateTenantHome,
  canonicalComponentStatus,
  compareStableVersions,
  componentProgressLabel,
  componentRowModel,
  hasComponentAttention,
  latestInfoFor,
} from "@/features/tenants/componentCatalog";

function row(overrides: Partial<ComponentRow> = {}): ComponentRow {
  return {
    kind: "node",
    supports_version: true,
    status: "installed",
    version: "24.1.0",
    error: null,
    ...overrides,
  };
}

function snapshot(version: string | null, state: "available" | "unavailable" = "available") {
  return {
    checked_at: "2026-08-20T00:00:00Z",
    entries: [{ kind: "node", state, version, source: "nodejs.org", error: null }],
  } satisfies ComponentLatestSnapshot;
}

describe("stable version comparison", () => {
  it("orders exact three-part releases", () => {
    expect(compareStableVersions("24.2.0", "24.1.9")).toBe(1);
    expect(compareStableVersions("24.1.0", "24.1.0")).toBe(0);
    expect(compareStableVersions("1.0.0", "10.0.0")).toBe(-1);
  });

  it("refuses anything that is not an exact X.Y.Z release", () => {
    for (const [left, right] of [
      ["24.1", "24.1.0"],
      ["24.1.0-rc1", "24.1.0"],
      ["v24.1.0", "24.1.0"],
      ["24.01.0", "24.1.0"],
    ]) {
      expect(compareStableVersions(left, right), `${left} vs ${right}`).toBeNull();
    }
  });

  it("compares parts numerically beyond the safe integer range", () => {
    expect(compareStableVersions("1.0.99999999999999999999", "1.0.1")).toBe(1);
  });
});

describe("latest release observation", () => {
  it("stays silent for an up-to-date statusline definition", () => {
    const info = latestInfoFor(row({ supports_version: false, status: "installed" }), null);
    expect(info.label).toBe("");
    expect(info.updateAvailable).toBe(false);
  });

  it("reports a changed statusline definition", () => {
    expect(latestInfoFor(row({ supports_version: false, status: "modified" }), null).label).toBe(
      "Definition changed",
    );
  });

  it("reports an unavailable observation without offering an update", () => {
    const info = latestInfoFor(row(), snapshot(null, "unavailable"));
    expect(info.label).toBe("Latest unavailable");
    expect(info.updateAvailable).toBe(false);
  });

  it("offers an update only for a strictly newer release", () => {
    expect(latestInfoFor(row(), snapshot("24.2.0")).updateAvailable).toBe(true);
    expect(latestInfoFor(row(), snapshot("24.1.0")).updateAvailable).toBe(false);
    expect(latestInfoFor(row(), snapshot("24.0.0")).updateAvailable).toBe(false);
  });

  it("explains an incomparable pair instead of offering an update", () => {
    const info = latestInfoFor(row({ version: "24.1" }), snapshot("24.2.0"));
    expect(info.updateAvailable).toBe(false);
    expect(info.detail).toBe("The observed and current versions could not be compared.");
  });
});

describe("component row model", () => {
  it("turns a newer release into a split Update action", () => {
    const model = componentRowModel(row(), snapshot("24.2.0"));
    expect(model.primaryAction).toBe("Update");
    expect(model.specificVersionMode).toBe("update");
    expect(model.canSpecificVersion).toBe(true);
  });

  it("keeps an equal release quiet with no action", () => {
    const model = componentRowModel(row(), snapshot("24.1.0"));
    expect(model.primaryAction).toBeNull();
    expect(model.showLatest).toBe(false);
    expect(model.diagnostic).toBeNull();
  });

  it("offers Install with a version menu for a missing Component", () => {
    const model = componentRowModel(row({ status: "not-installed", version: null }), null);
    expect(model.primaryAction).toBe("Install");
    expect(model.canSpecificVersion).toBe(true);
  });

  it("exposes unmanaged state as diagnostic only", () => {
    const model = componentRowModel(row({ status: "unmanaged", version: null }), null);
    expect(model.primaryAction).toBeNull();
    expect(model.presentation.canRemove).toBe(false);
    expect(model.diagnostic).toContain("not owned by AIBox");
  });

  it("offers Retry inspection when state could not be read", () => {
    const model = componentRowModel(row({ status: null, error: "permission denied" }), null);
    expect(model.primaryAction).toBe("Retry inspection");
    expect(model.presentation.badgeTone).toBe("error");
    expect(model.diagnostic).toBe("permission denied");
  });

  it("restores a modified versioned Component but updates an unversioned one", () => {
    expect(componentRowModel(row({ status: "modified" }), null).primaryAction).toBe("Restore");
    expect(
      componentRowModel(row({ status: "modified", supports_version: false }), null).primaryAction,
    ).toBe("Update");
  });
});

describe("component row labels", () => {
  it("names the canonical installed state", () => {
    expect(canonicalComponentStatus(row({ status: "not-installed" }))).toBe("Not installed");
    expect(canonicalComponentStatus(row({ error: "boom" }))).toBe("Inspection error");
  });

  it("describes the running Operation", () => {
    expect(componentProgressLabel(row({ status: "not-installed" }), true)).toBe("Installing…");
    expect(componentProgressLabel(row({ status: "incomplete" }), true)).toBe("Repairing…");
    expect(componentProgressLabel(row({ status: "modified" }), true)).toBe("Restoring…");
    expect(componentProgressLabel(row(), false)).toBe("Removing…");
  });

  it("flags rows that need attention", () => {
    expect(hasComponentAttention(row({ status: "incomplete" }), null)).toBe(true);
    expect(hasComponentAttention(row(), snapshot("24.2.0"))).toBe(true);
    expect(hasComponentAttention(row(), snapshot("24.1.0"))).toBe(false);
  });
});

describe("tenant home abbreviation", () => {
  it("shortens a path inside the Host Home", () => {
    expect(abbreviateTenantHome("/home/test/.aibox/tenants/work", "/home/test")).toBe(
      "~/.aibox/tenants/work",
    );
    expect(abbreviateTenantHome("/home/test", "/home/test")).toBe("~");
    expect(abbreviateTenantHome("/var/lib/aibox", "/home/test")).toBe("/var/lib/aibox");
    expect(abbreviateTenantHome("/var/lib/aibox", null)).toBe("/var/lib/aibox");
  });
});
