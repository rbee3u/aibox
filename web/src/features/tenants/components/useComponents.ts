import { useCallback, useEffect, useRef } from "react";

import type { Operation } from "@/api/operations";
import type { ComponentRow, TenantApi } from "@/api/tenants";
import type { TenantRow } from "@/api/core";
import {
  HOST_COMPONENT_GROUPS,
  MANAGED_COMPONENT_GROUPS,
  hasComponentAttention,
  hasComponentUpdate,
  isStatuslineComponent,
} from "@/features/tenants/components/componentCatalog";
import { useComponentCatalog } from "@/features/tenants/components/useComponentCatalog";
import { useComponentLatest } from "@/features/tenants/components/useComponentLatest";
import { useComponentMenu } from "@/features/tenants/components/useComponentMenu";
import { useComponentMutations } from "@/features/tenants/components/useComponentMutations";
import { tenantSelectionValue } from "@/domain/tenant";

interface ComponentOptions {
  api: TenantApi;
  loadTenants: () => Promise<TenantRow[] | null>;
  operation?: Operation | null;
  onOperation?: (operation: Operation) => void;
  selected: TenantRow | null;
  /** Owns the read error: a Component catalog read that succeeds clears it. */
  setReadError: (error: string | null) => void;
  /** Owns an action the user asked for; a background read never clears it. */
  reportActionFailure: (title: string, cause: unknown) => void;
}

/** Compose Component observation, menu state, and mutation policy for one Tenant. */
export function useComponents({
  api,
  loadTenants,
  operation,
  onOperation,
  selected,
  setReadError,
  reportActionFailure,
}: ComponentOptions) {
  const refreshedOperation = useRef<string | null>(null);
  const {
    close: closeComponentMenu,
    menuPosition: componentMenuPosition,
    menuRef: componentMenuRef,
    open: openComponentMenu,
    openMenu,
    registerButton: registerComponentMenuButton,
    registerItem: registerComponentMenuItem,
    toggle: toggleComponentMenu,
  } = useComponentMenu();
  const {
    components,
    load: loadComponentCatalog,
    loading: loadingComponents,
    loadedTenantKey,
  } = useComponentCatalog(api, setReadError);
  const {
    check: checkLatest,
    checking: checkingLatest,
    snapshot: latestSnapshot,
  } = useComponentLatest(api, (message) =>
    reportActionFailure("Couldn’t check for Component updates", message),
  );
  const selectedKey = selected ? tenantSelectionValue(selected) : null;
  const componentCatalogLoading =
    loadingComponents || (selectedKey !== null && loadedTenantKey !== selectedKey);
  const visibleComponents = componentCatalogLoading ? [] : components;
  const componentTotalCount = selected?.kind === "host" ? 2 : 8;
  const installedComponentCount = visibleComponents.filter(
    (row) => row.status === "installed" || row.status === "modified",
  ).length;
  const outdatedComponentCount = visibleComponents.filter((row) =>
    hasComponentUpdate(row, latestSnapshot),
  ).length;
  const differingComponentCount = visibleComponents.filter(
    (row) => row.status === "modified",
  ).length;
  const issueComponentCount = visibleComponents.filter(
    (row) => hasComponentAttention(row) && row.status !== "modified",
  ).length;
  const groupsToUse = selected?.kind === "host" ? HOST_COMPONENT_GROUPS : MANAGED_COMPONENT_GROUPS;
  const statuslinesWithoutParent =
    selected?.kind === "host"
      ? []
      : visibleComponents.filter(
          (row) =>
            isStatuslineComponent(row.kind) &&
            !visibleComponents.some(
              (parent) => parent.kind === (row.kind === "codex-statusline" ? "codex" : "claude"),
            ),
        );
  const componentGroups = groupsToUse
    .map((group) => {
      const rows = group.kinds
        .map((kind) => visibleComponents.find((row) => row.kind === kind))
        .filter((row): row is ComponentRow => Boolean(row));
      if (group.id === "agents" && statuslinesWithoutParent.length > 0) {
        rows.push(...statuslinesWithoutParent);
      }
      return { ...group, rows };
    })
    .filter((group) => group.rows.length > 0);
  const loadComponents = useCallback(
    async (target: TenantRow | null, showLoading = false) => {
      if (showLoading) closeComponentMenu();
      const rows = await loadComponentCatalog(target, showLoading);
      if (rows) closeComponentMenu();
    },
    [closeComponentMenu, loadComponentCatalog],
  );

  const actions = useComponentMutations({
    api,
    selected,
    latestSnapshot,
    loadComponents,
    onOperation,
    reportActionFailure,
  });
  const { clearProgress } = actions;
  useEffect(() => {
    void loadComponents(selected, true);
  }, [loadComponents, selected]);

  useEffect(() => {
    if (!operation || operation.state === "running" || refreshedOperation.current === operation.id)
      return;
    refreshedOperation.current = operation.id;
    clearProgress();
    void loadTenants();
    void loadComponents(selected);
  }, [loadComponents, loadTenants, operation, selected, clearProgress]);

  async function checkForUpdates() {
    if (checkingLatest) return;
    await Promise.all([checkLatest(), loadComponents(selected)]);
  }

  // Grouped the way the Tenant view model consumes it, so the controller can
  // spread these instead of forwarding three dozen fields by hand.
  return {
    /** Progress and busy state the page reads outside either group. */
    busy: actions.busy,
    componentActionProgress: actions.componentActionProgress,
    loadComponents,
    components: {
      allComponents: visibleComponents,
      checkingLatest,
      checkForUpdates,
      componentCatalogLoading,
      componentGroups,
      closeComponentMenu,
      componentMenuPosition,
      componentMenuRef,
      componentTotalCount,
      differingComponentCount,
      installedComponentCount,
      installComponent: actions.installComponent,
      issueComponentCount,
      outdatedComponentCount,
      latestSnapshot,
      openComponentMenu,
      openMenu,
      openSpecificVersion: actions.openSpecificVersion,
      registerComponentMenuButton,
      registerComponentMenuItem,
      submitSpecificVersion: actions.submitSpecificVersion,
      toggleComponentMenu,
    },
    dialogs: actions.dialogs,
  };
}
