import { useState } from "react";

import type { ConfigApi, ConfigListData } from "@/api/configs";
import type { AgentKind } from "@/domain/agent";
import type { TenantSelection } from "@/domain/tenant";
import {
  configLocation,
  namedConfigName,
  type ConfigSelection,
} from "@/features/common/routes/configs";
import type { ConfigApplyTarget, ConfigDeleteTarget } from "@/features/configs/configWorkflow";
import type { ConfigCatalogLoadKind } from "@/features/configs/viewTypes";
import { messageOf } from "@/shared/lib/errors";
import type { ModuleLocationChange } from "@/shared/lib/navigation";

interface ConfigCrudOptions {
  agent: AgentKind;
  api: Pick<ConfigApi, "applyConfig" | "createConfig" | "deleteConfigs">;
  currentSelection: boolean;
  file: string | null;
  loadCatalog: (kind?: ConfigCatalogLoadKind) => Promise<ConfigListData | null>;
  onLocationChange: ModuleLocationChange;
  operationRunning: boolean;
  onBusyChange: (busy: boolean) => void;
  onSelectionRecovery: (remaining: Set<string>, resume: boolean) => void;
  onSelectionReset: () => void;
  reloadFiles: (files: string[]) => void;
  requestEditorAction: (action: () => void | Promise<void>) => void;
  selection: ConfigSelection;
  selectionMode: boolean;
  reportActionFailure: (title: string, cause: unknown) => void;
  tenant: TenantSelection;
}

export function useConfigCrud({
  agent,
  api,
  currentSelection,
  file,
  loadCatalog,
  onLocationChange,
  operationRunning,
  onBusyChange,
  onSelectionRecovery,
  onSelectionReset,
  reloadFiles,
  requestEditorAction,
  selection,
  selectionMode,
  reportActionFailure,
  tenant,
}: ConfigCrudOptions) {
  const [newName, setNewName] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<ConfigDeleteTarget | null>(null);
  const [applyTarget, setApplyTarget] = useState<ConfigApplyTarget | null>(null);
  const [applyFeedback, setApplyFeedback] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  function requestDelete(names: string[]) {
    if (names.length === 0) return;
    requestEditorAction(() => setDeleteTarget({ names }));
  }

  function requestApply(name: string) {
    requestEditorAction(() => setApplyTarget({ name }));
  }

  function openCreateDialog() {
    requestEditorAction(() => {
      setCreateError(null);
      setCreateOpen(true);
    });
  }

  function changeNewName(name: string) {
    setNewName(name);
    setCreateError(null);
  }

  async function createConfig(name: string) {
    if (operationRunning || !name) return;
    onBusyChange(true);
    try {
      await api.createConfig(tenant, agent, name);
      setNewName("");
      setCreateError(null);
      setCreateOpen(false);
      await loadCatalog("background");
      onLocationChange(configLocation(tenant, agent, { current: false, config: name }, file));
    } catch (cause) {
      setCreateError(messageOf(cause));
    } finally {
      onBusyChange(false);
    }
  }

  async function applyConfig(name: string) {
    if (operationRunning) return;
    onBusyChange(true);
    setApplyFeedback(null);
    let applyError: unknown = null;
    try {
      await api.applyConfig(tenant, agent, name);
    } catch (cause) {
      applyError = cause;
    } finally {
      const refreshed = await loadCatalog("background");
      if (refreshed && currentSelection) {
        reloadFiles(refreshed.files);
      }
      setApplyTarget(null);
      if (applyError) {
        reportActionFailure(
          `Couldn’t apply Named Config ${name}`,
          `${messageOf(applyError)} Some Current Config files may already have been updated.`,
        );
      } else {
        setApplyFeedback(
          `Applied Named Config ${name} to Current Config. This is a one-time projection; it is not an Active Config.`,
        );
      }
      onBusyChange(false);
    }
  }

  async function deleteConfigs() {
    if (operationRunning || !deleteTarget || deleteTarget.names.length === 0) return;
    const requestedNames = deleteTarget.names;
    const wasSelectionMode = selectionMode;
    const inspectedName = namedConfigName(selection);
    onBusyChange(true);
    try {
      await api.deleteConfigs(tenant, agent, requestedNames);
      const deletedSelected = inspectedName !== null && requestedNames.includes(inspectedName);
      setDeleteTarget(null);
      onSelectionReset();
      if (deletedSelected) {
        onLocationChange(configLocation(tenant, agent, null), true);
      }
      await loadCatalog("background");
    } catch (cause) {
      setDeleteTarget(null);
      const refreshed = await loadCatalog("background");
      if (refreshed) {
        const remaining = requestedNames.filter((name) =>
          refreshed.configs.some((entry) => entry.name === name),
        );
        onSelectionRecovery(new Set(remaining), wasSelectionMode);
        if (inspectedName && !refreshed.configs.some((entry) => entry.name === inspectedName)) {
          onLocationChange(configLocation(tenant, agent, null), true);
        }
      }
      reportActionFailure(
        requestedNames.length === 1
          ? `Couldn’t delete Named Config ${requestedNames[0]}`
          : `Couldn’t delete ${requestedNames.length} Named Configs`,
        cause,
      );
    } finally {
      onBusyChange(false);
    }
  }

  return {
    applyFeedback,
    mutations: {
      applyConfig,
      createConfig,
      deleteConfigs,
      requestDelete,
    },
    dialogs: {
      applyTarget,
      cancelApply: () => setApplyTarget(null),
      cancelDelete: () => setDeleteTarget(null),
      changeNewName,
      closeCreateDialog: () => setCreateOpen(false),
      createError,
      createOpen,
      deleteTarget,
      newName,
      openCreateDialog,
      requestApply,
    },
  };
}
