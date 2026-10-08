import { useCallback, useEffect, useRef } from "react";
import type { AgentKind } from "@/domain/agent";
import type { TenantSelection } from "@/domain/tenant";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import type { AggregatedSessionData, SourcedSession } from "@/features/sessions/sessionSource";
import { sessionLocation } from "@/features/common/routes/sessions";

interface RouteChanges {
  previousSearch: string;
  previousRouteSourceKey: string;
  writtenSearch: string | null;
}

export function useSessionRouteWriter(
  search: string,
  routeSourceKey: string,
  onLocationChange: ModuleLocationChange,
) {
  const routeChanges = useRef<RouteChanges>({
    previousSearch: search,
    previousRouteSourceKey: routeSourceKey,
    writtenSearch: null,
  });
  const updateSessionLocation = useCallback(
    (query: URLSearchParams, replace = false) => {
      const suffix = query.toString();
      routeChanges.current.writtenSearch = suffix ? `?${suffix}` : "";
      onLocationChange(query, replace);
    },
    [onLocationChange],
  );
  const consumeRouteChange = useCallback((nextSearch: string, nextSourceKey: string) => {
    const previous = routeChanges.current;
    const changed = previous.previousSearch !== nextSearch;
    const sourceChanged = previous.previousRouteSourceKey !== nextSourceKey;
    const locallyWritten = previous.writtenSearch === nextSearch;
    routeChanges.current = {
      previousSearch: nextSearch,
      previousRouteSourceKey: nextSourceKey,
      writtenSearch: locallyWritten ? null : previous.writtenSearch,
    };
    return changed && !locallyWritten && !sourceChanged;
  }, []);
  return { updateSessionLocation, consumeRouteChange };
}

interface RouteInspectionOptions {
  consumeRouteChange: (search: string, sourceKey: string) => boolean;
  routeSourceKey: string;
  search: string;
  clearInspection: () => void;
  resetCatalog: () => void;
  load: () => Promise<AggregatedSessionData | null>;
  sessionId: string | null;
  data: AggregatedSessionData | null;
  loadingList: boolean;
  inspectedSession: () => SourcedSession | null;
  openSession: (row: SourcedSession, updateLocation?: boolean) => Promise<void>;
  tenant: TenantSelection;
  agent: AgentKind;
  updateSessionLocation: ModuleLocationChange;
}

/** Reconciles browser-owned selection with the catalog and detail stream. */
export function useSessionRouteInspection({
  consumeRouteChange,
  routeSourceKey,
  search,
  clearInspection,
  resetCatalog,
  load,
  sessionId,
  data,
  loadingList,
  inspectedSession,
  openSession,
  tenant,
  agent,
  updateSessionLocation,
}: RouteInspectionOptions) {
  useEffect(() => {
    if (!consumeRouteChange(search, routeSourceKey)) return;
    clearInspection();
    resetCatalog();
    void load();
  }, [clearInspection, load, resetCatalog, consumeRouteChange, routeSourceKey, search]);
  useEffect(() => {
    if (!sessionId) {
      if (inspectedSession()) clearInspection();
      return;
    }
    if (!data || loadingList) return;
    const row = data.sessions.find((candidate) => candidate.id === sessionId);
    if (row) {
      // URL-owned selection synchronizes the external detail stream lifecycle.
      if (inspectedSession()?.key !== row.key) void openSession(row, false);
      return;
    }
    // The refreshed catalog can invalidate a route-owned Session selection.
    clearInspection();
    updateSessionLocation(sessionLocation(tenant, agent), true);
  }, [
    agent,
    clearInspection,
    data,
    loadingList,
    openSession,
    sessionId,
    inspectedSession,
    tenant,
    updateSessionLocation,
  ]);
}
