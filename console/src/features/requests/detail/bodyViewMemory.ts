type BodyViewMode = "pretty" | "source";

export interface BodyViewMemory {
  mode: BodyViewMode;
  renderLarge: boolean;
  headersExpanded: boolean;
  expandedNodes: Set<string>;
  expandedStrings: Set<string>;
  expandedEvents: Set<number>;
  expandedEventRuns: Set<number>;
}

export function createBodyViewMemory(): BodyViewMemory {
  return {
    mode: "pretty",
    renderLarge: false,
    headersExpanded: false,
    expandedNodes: new Set(),
    expandedStrings: new Set(),
    expandedEvents: new Set(),
    expandedEventRuns: new Set(),
  };
}
