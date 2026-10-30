import { useCallback, useEffect, useState } from "react";
import type { Operation, OperationApi } from "@/api/operations";
import { readPreference, storePreference } from "@/shared/lib/preferences";

export type OperationConnection = "connecting" | "connected" | "reconnecting";

const DISMISSED_OPERATION_KEY = "aibox-console-operation-dismissed";

function readDismissedOperation(): string | null {
  return readPreference(DISMISSED_OPERATION_KEY) || null;
}

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

export function useOperationFeed(operations: OperationApi | null) {
  const [operation, setOperation] = useState<Operation | null>(null);
  const [connection, setConnection] = useState<OperationConnection>("connecting");
  const [dismissed, setDismissed] = useState<string | null>(readDismissedOperation);
  /** Space the fixed panel occupies, reported by the panel and reserved by the shell. */
  const [height, setHeight] = useState(0);

  useEffect(() => {
    if (!operations) return;
    return operations.subscribe({
      onConnection: setConnection,
      onOperation: (value, gap) => {
        setOperation((current) => mergeOperation(current, value, gap));
        if (value?.state === "running") setDismissed(null);
      },
    });
  }, [operations]);

  const record = useCallback((value: Operation) => {
    setOperation(value);
    setDismissed(null);
  }, []);

  return {
    operation,
    connection,
    height,
    setHeight,
    record,
    adopt: setOperation,
    dismiss: useCallback(() => {
      if (!operation) return;
      setDismissed(operation.id);
      // The Service keeps its latest Operation for reconnects, so the Console
      // must remember the acknowledgement or a reload would show it again.
      storePreference(DISMISSED_OPERATION_KEY, operation.id);
    }, [operation]),
    // Live progress must never be hidden by a stale acknowledgement.
    visible:
      operations && operation && (operation.state === "running" || dismissed !== operation.id)
        ? operation
        : null,
  };
}
