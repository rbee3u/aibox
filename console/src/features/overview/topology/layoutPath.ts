import type { TopologyNode, TreeIcon } from "@/features/overview/topology/coreTree";
import type { Tone } from "@/features/overview/viewTypes";

export const MIN_ZOOM = 0.65;
export const MAX_ZOOM = 1.5;

export type TopologyNodeKind = "entity" | "group" | "leaf";
export interface VisibleTopologyNode {
  node: TopologyNode;
  children: VisibleTopologyNode[];
  depth: number;
  open: boolean;
  branch: boolean;
  position: number;
  setSize: number;
}
export interface TopologyLayoutNode extends VisibleTopologyNode {
  x: number;
  y: number;
  width: number;
  height: number;
  kind: TopologyNodeKind;
}
export interface TopologyLayoutEdge {
  id: string;
  parentId: string;
  childId: string;
  path: string;
  tone: Tone;
}
export interface TopologyLayout {
  width: number;
  height: number;
  nodes: TopologyLayoutNode[];
  edges: TopologyLayoutEdge[];
}
export interface TopologyMetrics {
  layoutWidth: number;
  viewportWidth: number;
}
export function clampZoom(value: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round(value * 10) / 10));
}
export function fitTopologyZoom(canvasWidth: number, viewportWidth: number): number {
  return Math.min(1, clampZoom((viewportWidth - 32) / canvasWidth));
}
export function visibleTopology(
  node: TopologyNode,
  expanded: ReadonlySet<string>,
  forcedExpanded: ReadonlySet<string>,
  depth = 0,
  position = 1,
  setSize = 1,
): VisibleTopologyNode {
  const branch = node.children.length > 0;
  const open = node.id === "service" || expanded.has(node.id) || forcedExpanded.has(node.id);
  const children = open
    ? node.children.map((child, index) =>
        visibleTopology(
          child,
          expanded,
          forcedExpanded,
          depth + 1,
          index + 1,
          node.children.length,
        ),
      )
    : [];
  return { node, children, depth, open, branch, position, setSize };
}
export function layoutTopology(root: VisibleTopologyNode, availableWidth: number): TopologyLayout {
  const PADDING_X = 32;
  const PADDING_Y = 28;
  const MIN_LEVEL_GAP = 72;
  const MAX_LEVEL_GAP = 220;
  const SUBTREE_GAP = 20;
  const levels: VisibleTopologyNode[][] = [];
  const visitLevels = (entry: VisibleTopologyNode) => {
    (levels[entry.depth] ??= []).push(entry);
    for (const child of entry.children) visitLevels(child);
  };
  visitLevels(root);
  const levelWidths = levels.map((entries) =>
    Math.max(...entries.map((entry) => topologyNodeSize(entry.node.icon).width)),
  );
  const widthWithoutGaps = levelWidths.reduce((total, width) => total + width, PADDING_X * 2);
  const gapCount = Math.max(0, levelWidths.length - 1);
  const levelGap = gapCount
    ? Math.min(
        MAX_LEVEL_GAP,
        Math.max(MIN_LEVEL_GAP, (availableWidth - widthWithoutGaps) / gapCount),
      )
    : 0;
  const levelX: number[] = [];
  let nextX = PADDING_X;
  for (let depth = 0; depth < levelWidths.length; depth += 1) {
    levelX.push(nextX);
    nextX += levelWidths[depth] + levelGap;
  }
  const subtreeHeights = new Map<string, number>();
  const measure = (entry: VisibleTopologyNode): number => {
    const ownHeight = topologyNodeSize(entry.node.icon).height;
    if (entry.children.length === 0) {
      subtreeHeights.set(entry.node.id, ownHeight);
      return ownHeight;
    }
    const childHeight =
      entry.children.reduce((total, child) => total + measure(child), 0) +
      SUBTREE_GAP * (entry.children.length - 1);
    const height = Math.max(ownHeight, childHeight);
    subtreeHeights.set(entry.node.id, height);
    return height;
  };
  const rootHeight = measure(root);
  const nodes: TopologyLayoutNode[] = [];
  const place = (entry: VisibleTopologyNode, top: number) => {
    const size = topologyNodeSize(entry.node.icon);
    const subtreeHeight = subtreeHeights.get(entry.node.id) ?? size.height;
    nodes.push({
      ...entry,
      x: levelX[entry.depth],
      y: top + (subtreeHeight - size.height) / 2,
      width: size.width,
      height: size.height,
      kind: size.kind,
    });
    if (entry.children.length === 0) return;
    const childrenHeight =
      entry.children.reduce((total, child) => total + (subtreeHeights.get(child.node.id) ?? 0), 0) +
      SUBTREE_GAP * (entry.children.length - 1);
    let childTop = top + (subtreeHeight - childrenHeight) / 2;
    for (const child of entry.children) {
      place(child, childTop);
      childTop += (subtreeHeights.get(child.node.id) ?? 0) + SUBTREE_GAP;
    }
  };
  place(root, PADDING_Y);
  const nodeById = new Map(nodes.map((node) => [node.node.id, node]));
  const edges = nodes.flatMap((child) => {
    if (!child.node.parentId) return [];
    const parent = nodeById.get(child.node.parentId);
    if (!parent) return [];
    const startX = parent.x + parent.width;
    const startY = parent.y + parent.height / 2;
    const endX = child.x;
    const endY = child.y + child.height / 2;
    const curve = Math.max(28, (endX - startX) / 2);
    return [
      {
        id: `${parent.node.id}->${child.node.id}`,
        parentId: parent.node.id,
        childId: child.node.id,
        path: `M ${startX} ${startY} C ${startX + curve} ${startY}, ${endX - curve} ${endY}, ${endX} ${endY}`,
        tone: child.node.tone,
      },
    ];
  });
  return {
    width: Math.max(availableWidth, nextX - levelGap + PADDING_X),
    height: Math.max(420, rootHeight + PADDING_Y * 2),
    nodes,
    edges,
  };
}
export function topologyNodeSize(icon: TreeIcon): {
  width: number;
  height: number;
  kind: TopologyNodeKind;
} {
  if (["service", "host", "tenant", "codex", "claude"].includes(icon)) {
    return { width: 184, height: 58, kind: "entity" };
  }
  if (["config", "component"].includes(icon)) {
    return { width: 160, height: 38, kind: "leaf" };
  }
  return { width: 168, height: 46, kind: "group" };
}
export function topologyPath(root: TopologyNode, target: string): Set<string> {
  const path = new Set<string>();
  const visit = (node: TopologyNode): boolean => {
    if (node.id === target) {
      path.add(node.id);
      return true;
    }
    if (node.children.some(visit)) {
      path.add(node.id);
      return true;
    }
    return false;
  };
  visit(root);
  return path;
}
export function collectVisibleNodes(
  root: TopologyNode,
  expanded: Set<string>,
  forced: Set<string>,
  parentId: string | null = null,
): Array<{
  node: TopologyNode;
  parentId: string | null;
}> {
  const result = [{ node: root, parentId }];
  const open = root.id === "service" || expanded.has(root.id) || forced.has(root.id);
  if (open) {
    for (const child of root.children)
      result.push(...collectVisibleNodes(child, expanded, forced, root.id));
  }
  return result;
}
