import { useCallback, useEffect, useState } from "react";
import type { ConnectedControlApi } from "@/api/connect";
import type { Operation } from "@/api/operations";

export type OperationConnection = "connecting" | "connected" | "reconnecting";

/**
 * Tracks the latest Management Operation for the whole Console. Log frames
 * arrive incrementally, so a continuing Operation merges new sequences into the
 * snapshot it already holds; a reported gap or a different Operation replaces it.
 */
export function mergeOperation(
  current: Operation | null,
  incoming: Operation | null,
  gap: boolean,
): Operation | null {
  if (!incoming || !current || current.id !== incoming.id || gap) return incoming;
  const logs = new Map(current.logs.map((entry) => [entry.sequence, entry]));
  for (const entry of incoming.logs) logs.set(entry.sequence, entry);
  return {
    ...incoming,
    logs: [...logs.values()]
      .filter((entry) => entry.sequence >= incoming.first_sequence)
      .sort((left, right) => left.sequence - right.sequence),
  };
}

export function useOperationFeed(api: ConnectedControlApi | null) {
  const [operation, setOperation] = useState<Operation | null>(null);
  const [connection, setConnection] = useState<OperationConnection>("connecting");
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    if (!api) return;
    return api.operations.subscribe({
      onConnection: setConnection,
      onOperation: (value, gap) => {
        setOperation((current) => mergeOperation(current, value, gap));
        if (value?.state === "running") setDismissed(null);
      },
    });
  }, [api]);

  const record = useCallback((value: Operation) => {
    setOperation(value);
    setDismissed(null);
  }, []);

  return {
    operation,
    connection,
    expanded,
    setExpanded,
    record,
    adopt: setOperation,
    dismiss: useCallback(() => setDismissed(operation?.id ?? null), [operation]),
    visible: api && operation && dismissed !== operation.id ? operation : null,
  };
}
