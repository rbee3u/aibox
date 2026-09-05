import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { consoleModules, moduleFromPath, modulePath } from "@/app/routing/modules";
import type { ModuleId, ModuleLocationChange } from "@/shared/lib/navigation";

export interface RouteSnapshot {
  module: ModuleId;
  search: string;
}

function currentRoute(): RouteSnapshot {
  return { module: moduleFromPath(window.location.pathname), search: window.location.search };
}

function currentLocation(): string {
  return `${window.location.pathname}${window.location.search}${window.location.hash}`;
}

function confirmDiscardedConfig(): boolean {
  return window.confirm("Discard unsaved Config changes and continue?");
}

/**
 * Owns the Console's only `history` and `popstate` integration. Pages receive an
 * immutable route snapshot plus a writer for their own query, so no page
 * subscribes to browser history itself.
 *
 * A module holding unsaved edits can mark itself dirty; in-app navigation is
 * then deferred until the caller resolves `pendingNavigation`. Configs uses the
 * same Unsaved changes dialog as in-module leaves. History and unload
 * navigation fall back to a native confirmation.
 */
export function useConsoleRouter() {
  const [route, setRoute] = useState<RouteSnapshot>(currentRoute);
  const [pendingNavigation, setPendingNavigation] = useState<string | null>(null);
  const dirty = useRef(false);
  const acceptedLocation = useRef(currentLocation());

  useEffect(() => {
    const onPopState = () => {
      const next = currentLocation();
      if (dirty.current && !confirmDiscardedConfig()) {
        window.history.pushState(null, "", acceptedLocation.current);
        return;
      }
      acceptedLocation.current = next;
      setRoute(currentRoute());
    };
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  useEffect(() => {
    const preventDirtyUnload = (event: BeforeUnloadEvent) => {
      if (!dirty.current) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", preventDirtyUnload);
    return () => window.removeEventListener("beforeunload", preventDirtyUnload);
  }, []);

  const commitLocation = useCallback(
    (module: ModuleId, query?: URLSearchParams, replace = false) => {
      const next = modulePath(module, query);
      window.history[replace ? "replaceState" : "pushState"](null, "", next);
      acceptedLocation.current = next;
      const suffix = query?.toString();
      setRoute({ module, search: suffix ? `?${suffix}` : "" });
    },
    [],
  );

  // Pages keep `onLocationChange` in effect dependencies, so each module needs
  // one stable writer rather than a fresh closure per shell render.
  const locationChanges = useMemo(
    () =>
      Object.fromEntries(
        consoleModules.map((module) => [
          module.id,
          ((query, replace = false) =>
            commitLocation(module.id, query, replace)) satisfies ModuleLocationChange,
        ]),
      ) as Record<ModuleId, ModuleLocationChange>,
    [commitLocation],
  );

  const recordDirty = useCallback((value: boolean) => {
    dirty.current = value;
  }, []);

  const cancelPendingNavigation = useCallback(() => setPendingNavigation(null), []);

  return {
    route,
    commitLocation,
    locationChanges,
    recordDirty,
    isDirty: () => dirty.current,
    requestNavigation: setPendingNavigation,
    pendingNavigation,
    cancelPendingNavigation,
    acceptPendingNavigation: useCallback(() => {
      if (!pendingNavigation) return false;
      window.history.pushState(null, "", pendingNavigation);
      acceptedLocation.current = pendingNavigation;
      setRoute(currentRoute());
      setPendingNavigation(null);
      return true;
    }, [pendingNavigation]),
  };
}
