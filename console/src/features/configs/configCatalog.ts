import type { ConfigCatalogEntry, ConfigCustomProvider, PropagationOutcome } from "@/api/configs";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { TenantSelection } from "@/domain/tenant";
import { configTenantSelectionValue } from "@/features/configs/route";
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
export function propagationGroup(
  status: PropagationOutcome["status"],
): "updated" | "skipped" | "attention" {
  if (status === "updated") return "updated";
  if (status === "unchanged") return "skipped";
  return "attention";
}
export function propagationDetail(outcome: PropagationOutcome): string | null {
  switch (outcome.status) {
    case "newer":
      return `source ${outcome.source_last_refresh} · target ${outcome.target_last_refresh}`;
    case "conflict":
      return `last refresh ${outcome.last_refresh}`;
    case "invalid":
    case "failed":
      return outcome.reason;
    default:
      return null;
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
