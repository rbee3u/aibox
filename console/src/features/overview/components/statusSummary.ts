import type { OverviewData } from "@/api/overview";
import { abbreviateTenantHome } from "@/shared/lib/hostHome";

/** Narrow Overview replaces the fact row with this wrapping health summary. */
export function formatStatusSummary(
  overview: OverviewData | null,
  overviewError: string | null,
): { line: string; detail: string } {
  if (overviewError) {
    return { line: "Unavailable", detail: overviewError };
  }
  if (!overview) {
    return { line: "Loading", detail: "Connecting" };
  }
  const line = [
    "Running",
    `${overview.managed_tenants} Managed`,
    overview.host_available ? "Host available" : "Host unavailable",
    `Docker ${overview.docker.status}`,
    `Image ${overview.runtime_image.status}`,
  ].join(" · ");
  const root = abbreviateTenantHome(overview.service.aibox_root, overview.host_home);
  const detail = [overview.service.version, overview.service.listen, root].join(" · ");
  return { line, detail };
}
