import type {
  ComponentKind,
  ComponentLatestEntry,
  ComponentLatestSnapshot,
  ComponentRow,
} from "@/api/tenants";
import type { BrandName } from "@/shared/icons/brandIcons";

export type ComponentPrimaryAction = "Install" | "Update" | "Repair" | "Retry inspection";

export type ComponentBadgeTone = "warn" | "error";

export interface ComponentGroup {
  id: string;
  label: string;
  kinds: readonly ComponentKind[];
}

const COMPONENT_LABELS: Record<ComponentKind, string> = {
  node: "Node.js",
  codex: "Codex",
  claude: "Claude",
  python: "Python",
  "claude-statusline": "Claude Statusline",
  "codex-statusline": "Codex Statusline",
  rust: "Rust",
  go: "Go",
};

/** Presentation-only grouping; a Managed catalog shows Agents and Runtimes & Toolchains (with Statuslines merged into Agents). */
export const MANAGED_COMPONENT_GROUPS: readonly ComponentGroup[] = [
  { id: "agents", label: "Agents", kinds: ["codex", "claude"] },
  {
    id: "runtimes-toolchains",
    label: "Runtimes & Toolchains",
    kinds: ["node", "python", "rust", "go"],
  },
];

/** Host Tenant only manages host-level Statuslines. */
export const HOST_COMPONENT_GROUPS: readonly ComponentGroup[] = [
  { id: "statuslines", label: "Statuslines", kinds: ["codex-statusline", "claude-statusline"] },
];

export const AGENT_STATUSLINE_KIND: Record<
  "codex" | "claude",
  "codex-statusline" | "claude-statusline"
> = {
  codex: "codex-statusline",
  claude: "claude-statusline",
};

const COMPONENT_ACTION_MENU_WIDTHS: Record<"install" | "update", number> = {
  install: 136,
  update: 160,
};

type ComponentBrandKind = Exclude<ComponentKind, "codex-statusline" | "claude-statusline">;

export const COMPONENT_BRANDS: Record<ComponentBrandKind, BrandName> = {
  codex: "openai",
  claude: "claude",
  node: "nodejs",
  python: "python",
  rust: "rust",
  go: "go",
};

export function componentLabel(kind: ComponentKind): string {
  return COMPONENT_LABELS[kind];
}

/** Accepts a Tenants `component=` query value, or ignores an unknown kind. */
export function parseComponentKind(value: string | null | undefined): ComponentKind | null {
  if (!value) return null;
  return value in COMPONENT_LABELS ? (value as ComponentKind) : null;
}

export function isStatuslineComponent(
  kind: ComponentKind,
): kind is "codex-statusline" | "claude-statusline" {
  return kind === "codex-statusline" || kind === "claude-statusline";
}

/**
 * Compares two exact `X.Y.Z` versions, or reports `null` when either side is
 * not a comparable stable release. The installer stays responsible for whether
 * a version exists.
 */
export function compareStableVersions(left: string, right: string): number | null {
  const parse = (value: string) => {
    const parts = value.split(".");
    if (parts.length !== 3 || parts.some((part) => !/^(0|[1-9]\d*)$/.test(part))) {
      return null;
    }
    return parts.map(BigInt);
  };
  const leftParts = parse(left);
  const rightParts = parse(right);
  if (!leftParts || !rightParts) return null;
  for (let index = 0; index < leftParts.length; index += 1) {
    if (leftParts[index] !== rightParts[index]) {
      return leftParts[index] > rightParts[index] ? 1 : -1;
    }
  }
  return 0;
}

/** Validate an exact install version, or a strictly newer update version. */
export function componentVersionError(
  version: string,
  currentVersion?: string | null,
): string | null {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
    return "Enter a stable version in X.Y.Z form.";
  }
  if (currentVersion === undefined) return null;
  const comparison = currentVersion ? compareStableVersions(version, currentVersion) : null;
  if (comparison === 0) return `Version v${currentVersion} is already installed.`;
  if (comparison === -1) {
    return `Enter a version newer than v${currentVersion}. Remove the Component before installing a lower version.`;
  }
  return comparison === null ? "The installed version cannot be compared safely." : null;
}

export function latestEntryFor(
  snapshot: ComponentLatestSnapshot | null,
  kind: ComponentRow["kind"],
): ComponentLatestEntry | null {
  return snapshot?.entries.find((entry) => entry.kind === kind) ?? null;
}

/**
 * Node records LTS in `version` and the overall newest stable in `newest`.
 * An installed Node at or below the LTS tip is compared with LTS; anything
 * newer is compared with `newest`. Other Components leave `newest` empty.
 */
function effectiveLatestVersion(
  entry: Pick<ComponentLatestEntry, "version" | "newest">,
  installedVersion: string | null,
): string | null {
  if (!entry.version) return null;
  if (!installedVersion || !entry.newest) return entry.version;
  const installedAgainstLts = compareStableVersions(installedVersion, entry.version);
  if (installedAgainstLts !== null && installedAgainstLts <= 0) return entry.version;
  return entry.newest;
}

export interface ComponentLatestInfo {
  label: string;
  diagnostic: string | null;
  updateAvailable: boolean;
  installedVersion: string | null;
  latestVersion: string | null;
}

/**
 * Derives what a row should say about the observed Latest Release. Unversioned
 * statuslines describe their definition instead, and an equal version stays
 * silent so the row does not repeat itself.
 */
export function latestInfoFor(
  row: ComponentRow,
  snapshot: ComponentLatestSnapshot | null,
): ComponentLatestInfo {
  if (!row.supports_version) {
    return {
      label:
        row.status === "modified"
          ? "Definition changed"
          : row.status === "installed"
            ? ""
            : "No release version",
      diagnostic: null,
      updateAvailable: false,
      installedVersion: null,
      latestVersion: null,
    };
  }
  if (!snapshot) {
    return {
      label: "",
      diagnostic: null,
      updateAvailable: false,
      installedVersion: row.status === "installed" ? row.version : null,
      latestVersion: null,
    };
  }
  const entry = latestEntryFor(snapshot, row.kind);
  if (!entry || entry.state === "unavailable" || !entry.version) {
    return {
      label: "Latest unavailable",
      diagnostic: entry?.error ?? "The official source did not return a comparable stable version.",
      updateAvailable: false,
      installedVersion: row.status === "installed" ? row.version : null,
      latestVersion: null,
    };
  }
  if (row.status !== "installed" || !row.version) {
    return {
      label: `Latest release ${entry.version}`,
      diagnostic: null,
      updateAvailable: false,
      installedVersion: null,
      latestVersion: entry.version,
    };
  }
  const latestVersion = effectiveLatestVersion(entry, row.version) ?? entry.version;
  const comparison = compareStableVersions(latestVersion, row.version);
  const diagnostic =
    comparison === null
      ? "The observed and current versions could not be compared."
      : comparison === -1
        ? "The observed release is lower than the current version."
        : null;
  return {
    label: `Latest release ${latestVersion}`,
    diagnostic,
    updateAvailable: comparison === 1,
    installedVersion: row.version,
    latestVersion,
  };
}

export function canonicalComponentStatus(row: ComponentRow): string {
  if (row.error || !row.status) return "Inspection error";
  switch (row.status) {
    case "not-installed":
      return "Not installed";
    case "installed":
      return "Installed";
    case "incomplete":
      return "Incomplete";
    case "modified":
      return "Differs";
    case "unmanaged":
      return "Unmanaged";
    default:
      return row.status;
  }
}

export interface ComponentPresentation {
  stateLabel: string;
  stateBadge: string | null;
  badgeTone: ComponentBadgeTone | null;
  primaryAction: ComponentPrimaryAction | null;
  canRemove: boolean;
  diagnostic: string | null;
}

/**
 * Whether the row's Update overwrites state the Tenant may have edited by hand,
 * which is the one Component action that discards something without a way back.
 */
export function updateOverwritesLocalEdits(row: ComponentRow): boolean {
  return row.status === "modified";
}

/**
 * Keep healthy state quiet. `modified` statuslines say "Differs" because native
 * edits and a changed AIBox definition are indistinguishable. Unmanaged launchers
 * remain diagnostic-only.
 */
function componentPresentation(row: ComponentRow): ComponentPresentation {
  if (row.error || !row.status) {
    return {
      stateLabel: "",
      stateBadge: "Inspection error",
      badgeTone: "error",
      primaryAction: "Retry inspection",
      canRemove: false,
      diagnostic: row.error ?? "Component state could not be inspected safely.",
    };
  }
  switch (row.status) {
    case "not-installed":
      return {
        stateLabel: "Not installed",
        stateBadge: null,
        badgeTone: null,
        primaryAction: "Install",
        canRemove: false,
        diagnostic: null,
      };
    case "installed":
      return {
        stateLabel: "Installed",
        stateBadge: null,
        badgeTone: null,
        primaryAction: null,
        canRemove: true,
        diagnostic: null,
      };
    case "incomplete":
      return {
        stateLabel: "",
        stateBadge: "Incomplete",
        badgeTone: "warn",
        primaryAction: "Repair",
        canRemove: true,
        diagnostic: "Recognizable Component state is incomplete and can be repaired.",
      };
    case "modified":
      return {
        stateLabel: "",
        stateBadge: "Differs",
        badgeTone: "warn",
        primaryAction: "Update",
        canRemove: true,
        diagnostic: null,
      };
    case "unmanaged":
      return {
        stateLabel: "",
        stateBadge: "Unmanaged",
        badgeTone: "warn",
        primaryAction: null,
        canRemove: false,
        diagnostic: "Detected state is not owned by AIBox and will not be overwritten or deleted.",
      };
    default:
      return {
        stateLabel: "",
        stateBadge: row.status,
        badgeTone: "warn",
        primaryAction: null,
        canRemove: false,
        diagnostic: "This Component state is not recognized by this Console version.",
      };
  }
}

export interface ComponentRowModel {
  label: string;
  presentation: ComponentPresentation;
  latest: ComponentLatestInfo;
  /** Local state diagnostic, or the exceptional Latest Release observation. */
  diagnostic: string | null;
  primaryAction: ComponentPrimaryAction | null;
  specificVersionMode: "install" | "update";
  menuWidth: number;
  canSpecificVersion: boolean;
  showLatest: boolean;
}

/**
 * Derives everything one Component row displays. A checked versioned Component
 * with a higher Latest Release becomes an Update, and only an exceptional Latest
 * observation contributes a diagnostic.
 */
export function componentRowModel(
  row: ComponentRow,
  snapshot: ComponentLatestSnapshot | null,
): ComponentRowModel {
  const presentation = componentPresentation(row);
  const latest = latestInfoFor(row, snapshot);
  const primaryAction =
    row.status === "installed" && latest.updateAvailable ? "Update" : presentation.primaryAction;
  const specificVersionMode =
    row.status === "installed" && primaryAction === "Update" ? "update" : "install";
  return {
    label: componentLabel(row.kind),
    presentation,
    latest,
    diagnostic: presentation.diagnostic ?? latest.diagnostic,
    primaryAction,
    specificVersionMode,
    menuWidth: COMPONENT_ACTION_MENU_WIDTHS[specificVersionMode],
    canSpecificVersion:
      row.supports_version &&
      (row.status === "not-installed" ||
        row.status === "incomplete" ||
        specificVersionMode === "update"),
    showLatest:
      row.supports_version &&
      snapshot !== null &&
      (row.status !== "installed" || latest.latestVersion !== row.version),
  };
}

export function componentProgressLabel(row: ComponentRow, install: boolean): string {
  if (!install) return "Removing…";
  if (row.status === "incomplete") return "Repairing…";
  if (row.status === "modified" || row.status === "installed") return "Updating…";
  return "Installing…";
}

/**
 * Names a failed Component mutation the way its progress label named the
 * attempt, so a notice says which row failed and what it was doing.
 */
export function componentFailureTitle(row: ComponentRow, install: boolean): string {
  const label = componentLabel(row.kind);
  if (!install) return `Couldn’t remove ${label}`;
  if (row.status === "incomplete") return `Couldn’t repair ${label}`;
  if (row.status === "modified" || row.status === "installed") return `Couldn’t update ${label}`;
  return `Couldn’t install ${label}`;
}

/**
 * Updates are not faults; match the Service's Component attention semantics.
 */
export function hasComponentAttention(row: ComponentRow): boolean {
  if (row.error || !row.status) return true;
  return ["incomplete", "modified", "unmanaged"].includes(row.status);
}

/** Whether a newer release was observed for a Component that is otherwise fine. */
export function hasComponentUpdate(
  row: ComponentRow,
  snapshot: ComponentLatestSnapshot | null,
): boolean {
  if (hasComponentAttention(row)) return false;
  return latestInfoFor(row, snapshot).updateAvailable;
}

export function relativeTimeLabel(value: string): string {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return "Checked";
  const seconds = Math.round((timestamp - Date.now()) / 1000);
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ["year", 31_536_000],
    ["month", 2_592_000],
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ];
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  for (const [unit, size] of units) {
    if (Math.abs(seconds) >= size) return formatter.format(Math.round(seconds / size), unit);
  }
  return "just now";
}

/** Keeps a split-action menu inside the viewport, preferring below the trigger. */
export function componentMenuCoordinates(
  rect: DOMRect,
  width: number,
  height = 44,
): { top: number; left: number } {
  const left = Math.max(8, Math.min(rect.right - width, window.innerWidth - width - 8));
  const below = rect.bottom + 6;
  const top = below + height <= window.innerHeight - 8 ? below : Math.max(8, rect.top - height - 6);
  return { top, left };
}
