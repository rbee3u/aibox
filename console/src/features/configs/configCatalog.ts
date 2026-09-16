import type {
  ApplicationStatus,
  ConfigCatalogEntry,
  ConfigCustomProvider,
  ConfigDrift,
  PropagationOutcome,
} from "@/api/configs";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { TenantSelection } from "@/domain/tenant";
import { configTenantSelectionValue } from "@/features/configs/route";
import { driftCatalogLabel, formatTimestamp } from "@/shared/lib/format";
import type { IssueTone } from "@/shared/ui/IssueIndicator";

export interface ConfigIssuePresentation {
  tone: IssueTone;
  label: string;
  message: string;
  accessibleLabel: string;
}
export function configIssuePresentation(entry: ConfigCatalogEntry): ConfigIssuePresentation | null {
  if (entry.state === "ready") return null;
  const incomplete = entry.state === "incomplete";
  const tone = incomplete ? "warning" : "error";
  const label = incomplete ? "Incomplete Config" : "Invalid Config";
  const message =
    entry.detail ??
    (incomplete
      ? "Required Config files are missing. Use Repair to restore this Named Config."
      : "This Named Config cannot be safely used.");
  const toneLabel = incomplete ? "warning" : "error";
  return {
    tone,
    label,
    message,
    accessibleLabel: `Config ${toneLabel}: ${label}. ${message}`,
  };
}
export function configWarningPresentation(
  entry: ConfigCatalogEntry,
): ConfigIssuePresentation | null {
  if (entry.state !== "ready" || !entry.warnings?.length) return null;
  const message = entry.warnings.join(" ");
  return {
    tone: "warning",
    label: "Config warnings",
    message,
    accessibleLabel: `Config warning: ${message}`,
  };
}
export function configIssueDescriptionId(
  tenant: TenantSelection,
  agent: CodingAgentKind,
  name: string,
): string {
  return `config-issue-${configTenantSelectionValue(tenant).replace(":", "-")}-${agent}-${name}`;
}

export const lastAppliedDescriptionId = "config-last-applied";

export function lastAppliedMeta(applied: string, drift?: ConfigDrift): string {
  const base = `Last applied ${applied}`;
  return drift === "dirty" ? `${base} · differs` : base;
}

export { driftCatalogLabel };

export interface AppliedConfigPresentation {
  label: string;
  tone: "good" | "warning";
  variant: "inline" | "badge";
  /** Whether Apply still has work to do: a clean application converges to nothing. */
  applicable: boolean;
}
/**
 * How the Last Application source announces itself, on its catalog row and in
 * its detail header. Clean drift reads `Applied` — the fact a reader is after —
 * rather than the drift word; every other state keeps the shared drift label.
 */
export function appliedConfigPresentation(status: ApplicationStatus): AppliedConfigPresentation {
  if (status.drift === "clean") {
    return { label: "Applied", tone: "good", variant: "inline", applicable: false };
  }
  return {
    label: driftCatalogLabel(status.drift),
    tone: "warning",
    variant: "badge",
    applicable: true,
  };
}
export type PropagationGroup = "failed" | "updated" | "attention" | "skipped";
/** Display order: a write that failed is the one thing the reader believed happened and did not. */
export const propagationGroups: readonly PropagationGroup[] = [
  "failed",
  "updated",
  "attention",
  "skipped",
];
/**
 * `newer` is a target already fresher than the source, which is a skip by
 * rule rather than something to act on; `conflict` and `invalid` need a person.
 */
export function propagationGroup(status: PropagationOutcome["status"]): PropagationGroup {
  switch (status) {
    case "failed":
      return "failed";
    case "updated":
      return "updated";
    case "unchanged":
    case "newer":
      return "skipped";
    default:
      return "attention";
  }
}
export function propagationGroupHeading(group: PropagationGroup, preview: boolean): string {
  switch (group) {
    case "failed":
      return "Failed";
    case "updated":
      return preview ? "Will update" : "Updated";
    case "attention":
      return "Needs attention";
    default:
      return "Skipped";
  }
}
export interface PropagationStatusPresentation {
  tone: "good" | "neutral" | "warning" | "error";
  label: string;
}
export function propagationStatus(
  status: PropagationOutcome["status"],
  preview: boolean,
): PropagationStatusPresentation {
  switch (status) {
    case "updated":
      return { tone: "good", label: preview ? "Will update" : "Updated" };
    case "unchanged":
      return { tone: "neutral", label: "Unchanged" };
    case "newer":
      return { tone: "neutral", label: "Target newer" };
    case "conflict":
      return { tone: "warning", label: "Conflict" };
    case "invalid":
      return { tone: "warning", label: "Invalid" };
    default:
      return { tone: "error", label: "Failed" };
  }
}
/** The sentence beside the status: why this target got that outcome. */
export function propagationDetail(outcome: PropagationOutcome, preview = false): string | null {
  switch (outcome.status) {
    case "updated":
      return preview ? "Older credentials for the same account" : null;
    case "unchanged":
      return "Already holds the source credentials";
    case "newer":
      return `Refreshed ${formatTimestamp(outcome.target_last_refresh)}, after the source at ${formatTimestamp(outcome.source_last_refresh)}`;
    case "conflict":
      return `Different content with the same refresh time, ${formatTimestamp(outcome.last_refresh)}`;
    case "invalid":
    case "failed":
      return outcome.reason;
  }
}
export function requestProxyRoute(
  tenant: TenantSelection,
  listen: string | undefined,
): string | null {
  const port = listen?.match(/:(\d+)$/)?.[1];
  if (!port || port === "0") return null;
  return tenant.kind === "host"
    ? `http://127.0.0.1:${port}/`
    : `http://host.docker.internal:${port}/`;
}
export function splitRequestProxyValue(
  value: string,
  route: string | null,
): {
  upstream: string;
  routed: boolean;
} {
  if (!value || !route) return { upstream: value, routed: false };
  const knownRoute = /^https?:\/\/(?:127\.0\.0\.1|host\.docker\.internal):(\d+)\//i;
  const match = value.match(knownRoute);
  if (!match || match[1] === "0") return { upstream: value, routed: false };
  return { upstream: value.slice(match[0].length), routed: true };
}
export function comparableProvider(
  provider: ConfigCustomProvider | undefined,
): Pick<ConfigCustomProvider, "included" | "name" | "base_url"> | null {
  if (!provider) return null;
  return {
    included: provider.included,
    name: provider.name,
    base_url: provider.base_url,
  };
}
export function proxyValueIsValid(value: string): boolean {
  try {
    const url = new URL(value);
    return (url.protocol === "http:" || url.protocol === "https:") && Boolean(url.hostname);
  } catch {
    return false;
  }
}
