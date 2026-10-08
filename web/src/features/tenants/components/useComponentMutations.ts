import { useCallback, useId, useState } from "react";
import type { Operation } from "@/api/operations";
import type { ComponentRow, ComponentLatestSnapshot, TenantApi } from "@/api/tenants";
import type { TenantRow } from "@/api/core";
import type {
  ComponentRemoveTarget,
  ComponentUpdateTarget,
  ComponentSpecificVersionTarget,
  ComponentActionProgress,
} from "@/features/tenants/viewTypes";
import {
  componentVersionError,
  componentFailureTitle,
  componentProgressLabel,
  latestEntryFor,
  updateOverwritesLocalEdits,
} from "@/features/tenants/components/componentCatalog";
import { tenantSelection, tenantSelectionValue } from "@/domain/tenant";

interface Options {
  api: TenantApi;
  selected: TenantRow | null;
  latestSnapshot: ComponentLatestSnapshot | null;
  loadComponents: (target: TenantRow | null, showLoading?: boolean) => Promise<void>;
  onOperation?: (operation: Operation) => void;
  reportActionFailure: (title: string, cause: unknown) => void;
}
export function useComponentMutations({
  api,
  selected,
  latestSnapshot,
  loadComponents,
  onOperation,
  reportActionFailure,
}: Options) {
  const [busy, setBusy] = useState(false);
  const [componentActionProgress, setComponentActionProgress] =
    useState<ComponentActionProgress | null>(null);
  const [componentRemoveTarget, setComponentRemoveTarget] = useState<ComponentRemoveTarget | null>(
    null,
  );
  const [componentUpdateTarget, setComponentUpdateTarget] = useState<ComponentUpdateTarget | null>(
    null,
  );
  const [specificVersionTarget, setSpecificVersionTarget] =
    useState<ComponentSpecificVersionTarget | null>(null);
  const [specificVersion, setSpecificVersion] = useState("");
  const [specificVersionError, setSpecificVersionError] = useState<string | null>(null);
  const specificVersionTitleId = useId();
  const specificVersionHelpId = useId();
  const specificVersionValue = specificVersion.trim();
  const specificVersionValidationError =
    specificVersion.length > 0
      ? componentVersionError(
          specificVersionValue,
          specificVersionTarget?.mode === "update" ? specificVersionTarget.row.version : undefined,
        )
      : null;
  const specificVersionValid = specificVersionValue.length > 0 && !specificVersionValidationError;

  async function mutateComponent(
    row: ComponentRow,
    install: boolean,
    requestedVersion?: string | null,
  ): Promise<boolean> {
    if (!selected) return false;
    setBusy(true);
    setComponentActionProgress({
      tenantSelectionValue: tenantSelectionValue(selected),
      kind: row.kind,
      label: componentProgressLabel(row, install),
    });
    try {
      const latest = latestEntryFor(latestSnapshot, row.kind);
      let version: string | null = null;
      if (install) {
        if (requestedVersion !== undefined) {
          version = requestedVersion;
        } else if (row.supports_version && latest?.state === "available") {
          version = latest.version;
        }
      }
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
      reportActionFailure(componentFailureTitle(row, install), cause);
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

  function changeSpecificVersion(value: string) {
    setSpecificVersion(value);
    setSpecificVersionError(null);
  }

  function requestComponentRemove(row: ComponentRow, tenantLabel: string) {
    setComponentRemoveTarget({ row, tenantLabel });
  }

  async function removeComponent() {
    if (!componentRemoveTarget) return;
    await mutateComponent(componentRemoveTarget.row, false);
    setComponentRemoveTarget(null);
  }

  /**
   * Runs the row's primary install-side action. An Update that would overwrite
   * hand-edited state stops for confirmation first; everything else adds or
   * repairs and starts at once.
   */
  function installComponent(row: ComponentRow, requestedVersion?: string | null) {
    if (!selected) return;
    if (updateOverwritesLocalEdits(row)) {
      setComponentUpdateTarget({ row, tenantLabel: selected.display_name });
      return;
    }
    void mutateComponent(row, true, requestedVersion);
  }

  async function updateComponent() {
    if (!componentUpdateTarget) return;
    await mutateComponent(componentUpdateTarget.row, true);
    setComponentUpdateTarget(null);
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

  const clearProgress = useCallback(() => setComponentActionProgress(null), []);
  return {
    busy,
    componentActionProgress,
    clearProgress,
    installComponent,
    openSpecificVersion,
    submitSpecificVersion,
    dialogs: {
      cancelComponentRemove: () => setComponentRemoveTarget(null),
      cancelComponentUpdate: () => setComponentUpdateTarget(null),
      changeSpecificVersion,
      closeSpecificVersion: () => setSpecificVersionTarget(null),
      componentRemoveTarget,
      componentUpdateTarget,
      removeComponent,
      requestComponentRemove,
      specificVersion,
      specificVersionError,
      specificVersionHelpId,
      specificVersionTarget,
      specificVersionTitleId,
      specificVersionValid,
      specificVersionValidationError,
      updateComponent,
    },
  };
}
