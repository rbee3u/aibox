import { useCallback, useEffect, useId, useRef, useState } from "react";

import type { Operation } from "@/api/operations";
import type { ComponentKind, ComponentRow, TenantApi } from "@/api/tenants";
import type { TenantRow } from "@/api/core";
import {
  COMPONENT_GROUPS,
  compareStableVersions,
  componentProgressLabel,
  hasComponentAttention,
  latestEntryFor,
  tenantSelection,
} from "@/features/tenants/componentCatalog";
import { useComponentCatalog } from "@/features/tenants/mutation/useComponentCatalog";
import { useComponentLatest } from "@/features/tenants/mutation/useComponentLatest";
import { useComponentMenu } from "@/features/tenants/mutation/useComponentMenu";
import { tenantSelectionValueOf } from "@/features/tenants/route";
import type { TenantSelectionValue } from "@/domain/tenant";
import { messageOf } from "@/shared/lib/errors";

export type ComponentRemoveTarget = { row: ComponentRow; tenantLabel: string };
export type ComponentSpecificVersionTarget = {
  row: ComponentRow;
  tenantLabel: string;
  mode: "install" | "update";
};
export type ComponentActionProgress = {
  tenantSelectionValue: TenantSelectionValue;
  kind: ComponentKind;
  label: string;
};

interface ComponentActionOptions {
  api: TenantApi;
  loadTenants: () => Promise<TenantRow[] | null>;
  operation?: Operation | null;
  onOperation?: (operation: Operation) => void;
  selected: TenantRow | null;
  setError: (error: string | null) => void;
}

export function useComponentActions({
  api,
  loadTenants,
  operation,
  onOperation,
  selected,
  setError,
}: ComponentActionOptions) {
  const [busy, setBusy] = useState(false);
  const [componentActionProgress, setComponentActionProgress] =
    useState<ComponentActionProgress | null>(null);
  const [expandedComponents, setExpandedComponents] = useState<Set<string>>(new Set());
  const [componentRemoveTarget, setComponentRemoveTarget] = useState<ComponentRemoveTarget | null>(
    null,
  );
  const [specificVersionTarget, setSpecificVersionTarget] =
    useState<ComponentSpecificVersionTarget | null>(null);
  const [specificVersion, setSpecificVersion] = useState("");
  const [specificVersionError, setSpecificVersionError] = useState<string | null>(null);
  const refreshedOperation = useRef<string | null>(null);
  const specificVersionTitleId = useId();
  const specificVersionHelpId = useId();
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
    preserveNextError,
    tenantSelectionValue: componentsTenantSelectionValue,
  } = useComponentCatalog(api, setError);
  const {
    check: checkLatest,
    checking: checkingLatest,
    snapshot: latestSnapshot,
  } = useComponentLatest(api, setError);
  const selectedKey = selected ? tenantSelectionValueOf(selected) : null;
  const componentCatalogLoading =
    loadingComponents || (selectedKey !== null && componentsTenantSelectionValue !== selectedKey);
  const visibleComponents = componentCatalogLoading ? [] : components;
  const componentTotalCount = selected?.kind === "host" ? 2 : 8;
  const installedComponentCount = visibleComponents.filter(
    (row) => row.status === "installed" || row.status === "modified",
  ).length;
  const attentionComponentCount = visibleComponents.filter((row) =>
    hasComponentAttention(row, latestSnapshot),
  ).length;
  const componentGroups = COMPONENT_GROUPS.map((group) => ({
    ...group,
    rows: group.kinds
      .map((kind) => visibleComponents.find((row) => row.kind === kind))
      .filter((row): row is ComponentRow => Boolean(row)),
  })).filter((group) => group.rows.length > 0);
  const specificVersionValue = specificVersion.trim();
  const specificVersionFormatValid = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(
    specificVersionValue,
  );
  let specificVersionValidationError: string | null = null;
  if (specificVersion.length > 0 && !specificVersionFormatValid) {
    specificVersionValidationError = "Enter a stable version in X.Y.Z form.";
  } else if (specificVersionFormatValid && specificVersionTarget?.mode === "update") {
    const currentVersion = specificVersionTarget.row.version;
    const currentComparison = currentVersion
      ? compareStableVersions(specificVersionValue, currentVersion)
      : null;
    if (currentComparison === 0) {
      specificVersionValidationError = `Version v${currentVersion} is already installed.`;
    } else if (currentComparison === -1) {
      specificVersionValidationError = `Enter a version newer than v${currentVersion}. Remove the Component before installing a lower version.`;
    } else if (currentComparison === null) {
      specificVersionValidationError = "The installed version cannot be compared safely.";
    }
  }
  const specificVersionValid = specificVersionFormatValid && !specificVersionValidationError;

  const loadComponents = useCallback(
    async (target: TenantRow | null, showLoading = false) => {
      if (showLoading) {
        setExpandedComponents(new Set());
        closeComponentMenu();
      }
      const rows = await loadComponentCatalog(target, showLoading);
      if (rows) {
        setExpandedComponents(new Set());
        closeComponentMenu();
      }
    },
    [closeComponentMenu, loadComponentCatalog],
  );

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadComponents(selected, true);
  }, [loadComponents, selected]);

  useEffect(() => {
    if (!operation || operation.state === "running" || refreshedOperation.current === operation.id)
      return;
    refreshedOperation.current = operation.id;
    setComponentActionProgress(null);
    void loadTenants();
    void loadComponents(selected);
  }, [loadComponents, loadTenants, operation, selected]);

  async function checkForUpdates() {
    if (checkingLatest) return;
    await Promise.all([checkLatest(), loadComponents(selected)]);
  }

  async function mutateComponent(
    row: ComponentRow,
    install: boolean,
    requestedVersion?: string | null,
  ): Promise<boolean> {
    if (!selected) return false;
    setBusy(true);
    setComponentActionProgress({
      tenantSelectionValue: tenantSelectionValueOf(selected),
      kind: row.kind,
      label: componentProgressLabel(row, install),
    });
    try {
      const latest = latestEntryFor(latestSnapshot, row.kind);
      const version = !install
        ? null
        : requestedVersion === undefined
          ? row.supports_version && latest?.state === "available"
            ? latest.version
            : null
          : requestedVersion;
      const result = await api.mutateComponent(
        tenantSelection(selected),
        row.kind,
        install,
        version,
      );
      const operationStarted = result.kind === "operation" && Boolean(onOperation);
      if (result.kind === "operation") onOperation?.(result.operation);
      await loadComponents(selected);
      if (!operationStarted) setComponentActionProgress(null);
      return true;
    } catch (cause) {
      setError(messageOf(cause));
      setComponentActionProgress(null);
      return false;
    } finally {
      setBusy(false);
    }
  }

  function openSpecificVersion(row: ComponentRow, mode: ComponentSpecificVersionTarget["mode"]) {
    if (!selected) return;
    setSpecificVersionTarget({ row, tenantLabel: selected.display_name, mode });
    setSpecificVersion("");
    setSpecificVersionError(null);
  }

  function closeSpecificVersion() {
    setSpecificVersionTarget(null);
  }

  function changeSpecificVersion(value: string) {
    setSpecificVersion(value);
    setSpecificVersionError(null);
  }

  function toggleComponentExpanded(kind: ComponentKind) {
    setExpandedComponents((current) => {
      const next = new Set(current);
      if (!next.delete(kind)) next.add(kind);
      return next;
    });
  }

  function requestComponentRemove(row: ComponentRow, tenantLabel: string) {
    setComponentRemoveTarget({ row, tenantLabel });
  }

  function cancelComponentRemove() {
    setComponentRemoveTarget(null);
  }

  async function removeComponent() {
    if (!componentRemoveTarget) return;
    await mutateComponent(componentRemoveTarget.row, false);
    setComponentRemoveTarget(null);
  }

  async function submitSpecificVersion() {
    if (!specificVersionTarget || !specificVersionValid) return;
    const installed = await mutateComponent(specificVersionTarget.row, true, specificVersionValue);
    if (installed) {
      setSpecificVersionTarget(null);
    } else {
      setSpecificVersionError(
        `The specific version could not be ${specificVersionTarget.mode === "update" ? "updated" : "installed"}.`,
      );
    }
  }

  // Grouped the way the Tenant view model consumes it, so the controller can
  // spread these instead of forwarding three dozen fields by hand.
  return {
    /** Progress and busy state the page reads outside either group. */
    busy,
    componentActionProgress,
    preserveNextError,
    loadComponents,
    components: {
      attentionComponentCount,
      checkingLatest,
      checkForUpdates,
      componentCatalogLoading,
      componentGroups,
      closeComponentMenu,
      componentMenuPosition,
      componentMenuRef,
      componentTotalCount,
      installedComponentCount,
      isComponentExpanded: (kind: ComponentKind) => expandedComponents.has(kind),
      latestSnapshot,
      mutateComponent,
      openComponentMenu,
      openMenu,
      openSpecificVersion,
      registerComponentMenuButton,
      registerComponentMenuItem,
      submitSpecificVersion,
      toggleComponentExpanded,
      toggleComponentMenu,
    },
    dialogs: {
      cancelComponentRemove,
      changeSpecificVersion,
      closeSpecificVersion,
      componentRemoveTarget,
      removeComponent,
      requestComponentRemove,
      specificVersion,
      specificVersionError,
      specificVersionHelpId,
      specificVersionTarget,
      specificVersionTitleId,
      specificVersionValid,
      specificVersionValidationError,
    },
  };
}
