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

export function toggleBodyViewSet<T>(values: Set<T>, value: T): Set<T> {
  const next = new Set(values);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}
