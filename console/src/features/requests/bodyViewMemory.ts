type BodyViewMode = "pretty" | "source";

export interface BodyViewMemory {
  mode: BodyViewMode;
  renderLarge: boolean;
  expandedNodes: Set<string>;
  expandedStrings: Set<string>;
  expandedEvents: Set<number>;
}

export function createBodyViewMemory(): BodyViewMemory {
  return {
    mode: "pretty",
    renderLarge: false,
    expandedNodes: new Set(),
    expandedStrings: new Set(),
    expandedEvents: new Set(),
  };
}
