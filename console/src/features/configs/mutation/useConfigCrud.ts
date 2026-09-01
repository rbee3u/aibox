import type { Dispatch, SetStateAction } from "react";
import { useState } from "react";

import type { ConfigApi, ConfigListData } from "@/api/configs";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { TenantSelection } from "@/domain/tenant";
import {
  configLocation,
  type ConfigApplyTarget,
  type ConfigDeleteTarget,
  type ConfigSelection,
} from "@/features/configs/route";
import type { ConfigCatalogLoadKind } from "@/features/configs/viewTypes";
import { messageOf } from "@/shared/lib/errors";
import type { ModuleLocationChange } from "@/shared/lib/navigation";

interface ConfigCrudOptions {
  agent: CodingAgentKind;
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
  setError: Dispatch<SetStateAction<string | null>>;
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
  setError,
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

  function closeCreateDialog() {
    setCreateOpen(false);
  }

  function changeNewName(name: string) {
    setNewName(name);
    setCreateError(null);
  }

  function cancelApply() {
    setApplyTarget(null);
  }

  function cancelDelete() {
    setDeleteTarget(null);
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
    let applyError: string | null = null;
    try {
      await api.applyConfig(tenant, agent, name);
    } catch (cause) {
      applyError = `${messageOf(cause)} Some Current Config files may already have been updated.`;
    } finally {
      const refreshed = await loadCatalog("background");
      if (refreshed && currentSelection) {
        reloadFiles(refreshed.files);
      }
      setApplyTarget(null);
      setError(applyError);
      if (!applyError) {
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
    onBusyChange(true);
    try {
      await api.deleteConfigs(tenant, agent, requestedNames);
      const deletedSelected = !selection.current && requestedNames.includes(selection.config ?? "");
      setDeleteTarget(null);
      onSelectionReset();
      if (deletedSelected) {
        onLocationChange(configLocation(tenant, agent, null), true);
      }
      await loadCatalog("background");
    } catch (cause) {
      const deletionError = messageOf(cause);
      setDeleteTarget(null);
      const refreshed = await loadCatalog("background");
      if (refreshed) {
        const remaining = requestedNames.filter((name) =>
          refreshed.configs.some((entry) => entry.name === name),
        );
        onSelectionRecovery(new Set(remaining), wasSelectionMode);
        if (
          !selection.current &&
          !refreshed.configs.some((entry) => entry.name === selection.config)
        ) {
          onLocationChange(configLocation(tenant, agent, null), true);
        }
      }
      setError(deletionError);
    } finally {
      onBusyChange(false);
    }
  }

  // Grouped the way the Config view model consumes it, so the controller
  // spreads these rather than forwarding each field.
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
      cancelApply,
      cancelDelete,
      changeNewName,
      closeCreateDialog,
      createError,
      createOpen,
      deleteTarget,
      newName,
      openCreateDialog,
      requestApply,
    },
  };
}
