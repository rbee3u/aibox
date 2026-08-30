import type { TopologyData } from "@/api/overview";
import {
  tenantId,
  type NavigationTarget,
  type TopologyNode,
} from "@/features/overview/topology/coreTree";

export interface TopologySearchResult {
  matches: Set<string>;
  context: Set<string>;
  firstMatch: string | null;
}
export function structuralIds(data: TopologyData): Set<string> {
  const ids = new Set<string>();
  for (const tenant of data.tenants) {
    const base = tenantId(tenant);
    ids.add(base);
    for (const agent of tenant.agents) {
      const agentId = `${base}/agent:${agent.agent}`;
      ids.add(agentId);
      if (agent.named_configs.entries.length) ids.add(`${agentId}/named-configs`);
    }
    if (
      tenant.components.entries.some((entry) => entry.status !== "not-installed" || entry.error)
    ) {
      ids.add(`${base}/components`);
    }
  }
  return ids;
}
export function defaultExpansion(data: TopologyData): Set<string> {
  return structuralIds(data);
}
export function collectBranchIds(node: TopologyNode, result: Set<string>) {
  if (node.children.length) result.add(node.id);
  for (const child of node.children) collectBranchIds(child, result);
}
export function searchTopology(root: TopologyNode | null, query: string): TopologySearchResult {
  const matches = new Set<string>();
  const context = new Set<string>();
  const normalized = query.toLocaleLowerCase();
  let firstMatch: string | null = null;
  if (!root || !normalized) return { matches, context, firstMatch };
  const visit = (node: TopologyNode, ancestors: string[]) => {
    const text = `${node.label} ${node.detail ?? ""} ${node.title ?? ""}`.toLocaleLowerCase();
    if (text.includes(normalized)) {
      matches.add(node.id);
      firstMatch ??= node.id;
      for (const id of ancestors) context.add(id);
    }
    for (const child of node.children) visit(child, [...ancestors, node.id]);
  };
  visit(root, []);
  return { matches, context, firstMatch };
}
export function filterByAttention(node: TopologyNode): TopologyNode | null {
  const children = node.children
    .map(filterByAttention)
    .filter((child): child is TopologyNode => Boolean(child));
  const matches = node.tone === "warning" || node.tone === "error";
  return matches || children.length ? { ...node, children } : null;
}
export function emptyFilteredRoot(root: TopologyNode, detail: string): TopologyNode {
  return { ...root, detail, tone: "good", children: [] };
}
export function targetHref(target: NavigationTarget): string {
  const query = target.query?.toString();
  return `/_aibox/ui/${target.module}${query ? `?${query}` : ""}`;
}
