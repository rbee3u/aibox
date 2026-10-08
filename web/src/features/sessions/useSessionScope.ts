import { useCallback, useEffect } from "react";
import type { SessionApi } from "@/api/sessions";
import type { AgentKind } from "@/domain/agent";
import type { TenantSelection } from "@/domain/tenant";
import { useSessionCatalog } from "@/features/sessions/catalog/useSessionCatalog";
import type { SessionCatalogResult } from "@/features/sessions/sessionCatalog";
import type { SourcedSession } from "@/features/sessions/sessionSource";
import { messageOf } from "@/shared/lib/errors";

interface SessionScopeOptions {
  api: Pick<SessionApi, "listSessions">;
  tenant: TenantSelection;
  agent: AgentKind;
  abortDetailStream: () => void;
  clearInspection: () => void;
  inspectedSession: () => SourcedSession | null;
  replaceCurrent: (row: SourcedSession) => void;
  resetSelection: () => void;
  setError: (error: string | null) => void;
}

/** Coordinates catalog, inspection and selection within one Tenant/Agent scope. */
export function useSessionScope({
  api,
  tenant,
  agent,
  abortDetailStream,
  clearInspection,
  inspectedSession,
  replaceCurrent,
  resetSelection,
  setError,
}: SessionScopeOptions) {
  const reconcileCatalog = useCallback(
    (result: SessionCatalogResult) => {
      if (result.kind === "failed") {
        setError(`Couldn’t load Sessions: ${messageOf(result.cause)}`);
        resetSelection();
        return;
      }
      setError(null);
      const inspected = inspectedSession();
      if (inspected) {
        const refreshed = result.data.sessions.find((row) => row.key === inspected.key);
        if (refreshed) replaceCurrent(refreshed);
        else clearInspection();
      }
      if (result.data.warnings.length > 0) resetSelection();
    },
    [clearInspection, inspectedSession, replaceCurrent, resetSelection, setError],
  );
  const catalog = useSessionCatalog({ api, tenant, agent, onResult: reconcileCatalog });
  const { cancel, load, reset } = catalog;

  useEffect(() => {
    // A new source clears the old inspection and selection before starting its read.
    clearInspection();
    reset();
    setError(null);
    resetSelection();
    void load();
    return () => {
      cancel();
      abortDetailStream();
    };
  }, [abortDetailStream, cancel, clearInspection, load, reset, resetSelection, setError]);

  return catalog;
}
