import type { Dispatch } from "react";
import type { TenantApi } from "@/api/tenants";
import type { TenantRow } from "@/api/core";
import { DNS_LABEL_PATTERN, type TenantSelectionValue } from "@/domain/tenant";
import { tenantLocation } from "@/features/common/routes/tenants";

import {
  tenantRowSuccessor,
  type TenantWorkflowState,
  type TenantWorkflowAction,
} from "@/features/tenants/tenantWorkflow";
import { messageOf } from "@/shared/lib/errors";
import type { ModuleLocationChange } from "@/shared/lib/navigation";

interface TenantMutationsOptions {
  api: TenantApi;
  workflow: TenantWorkflowState;
  dispatchWorkflow: Dispatch<TenantWorkflowAction>;
  tenants: TenantRow[];
  sortedManagedTenants: Array<TenantRow & { kind: "managed"; name: string }>;
  loadTenants: () => Promise<TenantRow[] | null>;
  onLocationChange: ModuleLocationChange;
  focusTenantRow: (key: TenantSelectionValue) => boolean;
  reportActionFailure: (title: string, cause: unknown) => void;
}

/** Tenant lifecycle requests, partial failure recovery, and post-mutation focus. */
export function useTenantMutations({
  api,
  workflow,
  dispatchWorkflow,
  tenants,
  sortedManagedTenants,
  loadTenants,
  onLocationChange,
  focusTenantRow,
  reportActionFailure,
}: TenantMutationsOptions) {
  const { newName, selectionMode, deleteTarget } = workflow;
  const createNameValid = DNS_LABEL_PATTERN.test(newName);
  const createNameTaken = tenants.some((row) => row.kind === "managed" && row.name === newName);
  /**
   * Moves focus to a row once the reload that produced it has committed. The
   * dialog that just closed restores focus only when nothing else claimed it,
   * so a row focused here wins over the control that opened the dialog.
   */
  function focusTenantRowSoon(key: TenantSelectionValue) {
    window.requestAnimationFrame(() => {
      if (!focusTenantRow(key)) window.requestAnimationFrame(() => focusTenantRow(key));
    });
  }

  async function createTenant() {
    if (!createNameValid || createNameTaken) return;
    dispatchWorkflow({ type: "create_started" });
    try {
      await api.createTenant(newName);
      const created = newName;
      dispatchWorkflow({ type: "create_succeeded" });
      await loadTenants();
      const key = `managed:${created}` as TenantSelectionValue;
      onLocationChange(tenantLocation(key));
      focusTenantRowSoon(key);
    } catch (cause) {
      dispatchWorkflow({ type: "create_failed", message: messageOf(cause) });
    }
  }

  function requestTenantDelete(names: string[]) {
    if (names.length === 0) return;
    dispatchWorkflow({ type: "delete_requested", names });
  }

  async function deleteTenants() {
    if (!deleteTarget || deleteTarget.names.length === 0) return;
    const requestedNames = deleteTarget.names;
    const wasSelectionMode = selectionMode;
    const successor = tenantRowSuccessor(
      sortedManagedTenants.map((row) => row.name),
      requestedNames,
    );
    dispatchWorkflow({ type: "delete_started" });
    try {
      await api.deleteTenants(requestedNames);
      dispatchWorkflow({ type: "delete_succeeded" });
      await loadTenants();
      focusTenantRowSoon(successor ?? "host");
    } catch (cause) {
      const refreshed = await loadTenants();
      if (refreshed) {
        const remaining = requestedNames.filter((name) =>
          refreshed.some((row) => row.kind === "managed" && row.name === name),
        );
        dispatchWorkflow({
          type: "delete_failed",
          remaining: remaining.map((name) => `managed:${name}` as TenantSelectionValue),
          resumeSelection: wasSelectionMode,
        });
      } else {
        dispatchWorkflow({ type: "delete_failed", remaining: [], resumeSelection: false });
      }
      reportActionFailure(
        requestedNames.length === 1
          ? `Couldn’t delete Tenant ${requestedNames[0]}`
          : `Couldn’t delete ${requestedNames.length} Tenants`,
        cause,
      );
    }
  }

  return { createTenant, deleteTenants, requestTenantDelete, createNameValid, createNameTaken };
}
