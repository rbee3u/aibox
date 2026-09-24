import { describe, expect, it } from "vitest";
import type { ComponentLatestSnapshot, ComponentRow } from "@/api/tenants";
import {
  canonicalComponentStatus,
  compareStableVersions,
  componentProgressLabel,
  componentRowModel,
  hasComponentAttention,
  hasComponentUpdate,
  latestInfoFor,
  parseComponentKind,
  updateOverwritesLocalEdits,
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

function snapshot(
  version: string | null,
  state: "available" | "unavailable" = "available",
  newest: string | null = null,
) {
  return {
    checked_at: "2026-08-20T00:00:00Z",
    entries: [{ kind: "node", state, version, newest, source: "nodejs.org", error: null }],
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

  it("compares an installed Node at or below the LTS tip against LTS, not newest", () => {
    const current = latestInfoFor(
      row({ version: "24.19.0" }),
      snapshot("24.21.0", "available", "26.8.2"),
    );
    expect(current.latestVersion).toBe("24.21.0");
    expect(current.updateAvailable).toBe(true);
    expect(current.label).toBe("Latest release 24.21.0");

    const olderLine = latestInfoFor(
      row({ version: "22.17.0" }),
      snapshot("24.21.0", "available", "26.8.2"),
    );
    expect(olderLine.latestVersion).toBe("24.21.0");
    expect(olderLine.updateAvailable).toBe(true);
  });

  it("compares an installed Node newer than LTS against the newest stable release", () => {
    const outdated = latestInfoFor(
      row({ version: "26.1.0" }),
      snapshot("24.21.0", "available", "26.8.2"),
    );
    expect(outdated.latestVersion).toBe("26.8.2");
    expect(outdated.updateAvailable).toBe(true);
    expect(outdated.label).toBe("Latest release 26.8.2");

    const current = latestInfoFor(
      row({ version: "26.8.2" }),
      snapshot("24.21.0", "available", "26.8.2"),
    );
    expect(current.updateAvailable).toBe(false);
    expect(current.detail).toBe("Up to date.");
  });

  it("treats equal LTS and newest as a single observed release", () => {
    const info = latestInfoFor(
      row({ version: "24.21.0" }),
      snapshot("24.21.0", "available", "24.21.0"),
    );
    expect(info.latestVersion).toBe("24.21.0");
    expect(info.updateAvailable).toBe(false);
  });

  it("keeps an uninstalled Node on the LTS observation", () => {
    const info = latestInfoFor(
      row({ status: "not-installed", version: null }),
      snapshot("24.21.0", "available", "26.8.2"),
    );
    expect(info.latestVersion).toBe("24.21.0");
    expect(info.updateAvailable).toBe(false);
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

  /*
   * Only statuslines report `modified`, and the Console cannot tell a hand
   * edit from a definition that changed upstream, so the row says neither:
   * it says the two differ, and that Update rewrites it.
   */
  it("says a differing statusline differs and that Update overwrites it", () => {
    const model = componentRowModel(row({ status: "modified", supports_version: false }), null);
    expect(model.primaryAction).toBe("Update");
    expect(model.presentation.stateBadge).toBe("Differs");
    expect(model.diagnostic).toBeNull();
    expect(updateOverwritesLocalEdits(row({ status: "modified", supports_version: false }))).toBe(
      true,
    );
    expect(updateOverwritesLocalEdits(row({ status: "installed" }))).toBe(false);
    expect(updateOverwritesLocalEdits(row({ status: "incomplete" }))).toBe(false);
  });
});

describe("parseComponentKind", () => {
  it("accepts known kinds and ignores everything else", () => {
    expect(parseComponentKind("claude-statusline")).toBe("claude-statusline");
    expect(parseComponentKind("rust")).toBe("rust");
    expect(parseComponentKind("nope")).toBeNull();
    expect(parseComponentKind(null)).toBeNull();
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
    expect(componentProgressLabel(row({ status: "modified" }), true)).toBe("Updating…");
    expect(componentProgressLabel(row(), false)).toBe("Removing…");
  });

  /*
   * The two used to be one predicate, so a Tenant whose Components were all
   * healthy but had newer releases upstream reported them as issues — in the
   * warning colour, and disagreeing with Overview for the same Tenant.
   */
  it("flags rows that need attention, which an available update is not", () => {
    expect(hasComponentAttention(row({ status: "incomplete" }))).toBe(true);
    expect(hasComponentAttention(row({ status: "modified" }))).toBe(true);
    expect(hasComponentAttention(row({ status: "unmanaged" }))).toBe(true);
    expect(hasComponentAttention(row({ error: "boom" }))).toBe(true);
    expect(hasComponentAttention(row())).toBe(false);
  });

  it("reports an available update separately, and never for a row already in trouble", () => {
    expect(hasComponentUpdate(row(), snapshot("24.2.0"))).toBe(true);
    expect(hasComponentUpdate(row(), snapshot("24.1.0"))).toBe(false);
    expect(hasComponentUpdate(row(), null)).toBe(false);
    /*
     * An installed row whose inspection failed is already an issue, and a newer
     * release is still observable for it — without the guard the same Component
     * would be counted in both numbers at once.
     */
    expect(hasComponentAttention(row({ error: "boom" }))).toBe(true);
    expect(hasComponentUpdate(row({ error: "boom" }), snapshot("24.2.0"))).toBe(false);
  });
});
