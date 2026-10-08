import type { ConfigEditorSession } from "@/features/configs/editor/configEditorSession";
import type { Dispatch, RefObject, SetStateAction } from "react";
import type {
  ConfigApi,
  ConfigListData,
  PropagationPreview,
  PropagationReport,
} from "@/api/configs";
import type { AgentKind } from "@/domain/agent";
import type { TenantSelection, TenantSelectionValue } from "@/domain/tenant";

import type { ConfigSelection } from "@/features/common/routes/configs";
import type {
  ConfigApplyTarget,
  ConfigDeleteTarget,
  ConfigPendingAction,
} from "@/features/configs/configWorkflow";

import type { NotificationItemData, NotificationSource } from "@/shared/ui/notificationTypes";
import type { SelectionOption } from "@/shared/ui/SelectionMenu";
import type { Operation } from "@/api/operations";
import type { ModuleLocationChange } from "@/shared/lib/navigation";

export interface ConfigPageProps {
  api: ConfigApi;
  operation?: Operation | null;
  search: string;
  onDirtyChange?: (dirty: boolean) => void;
  onCancelLeave?: () => void;
  onContinueLeave?: () => void | Promise<void>;
  onLocationChange: ModuleLocationChange;
  pendingLeave?: boolean;
}

/**
 * Types more than one Configs concern shares.
 *
 * Catalog loading and Config mutations share this load classification.
 */
export type ConfigCatalogLoadKind = "initial" | "refresh" | "background";

export interface ConfigViewModel {
  catalog: {
    agent: AgentKind;
    agentOptions: SelectionOption<AgentKind>[];
    catalog: ConfigListData | null;
    catalogError: string | null;
    configFiles: string[];
    configSelectionLabel: string;
    configTenantLabel: string;
    fileStatuses: Record<string, ConfigFileStatus>;
    loadCatalog: (kind?: ConfigCatalogLoadKind) => Promise<ConfigListData | null>;
    loadingCatalog: boolean;
    loadingTenants: boolean;
    managedTenantMissing: boolean;
    refreshing: boolean;
    refreshConfigs: () => Promise<void>;
    retryTenants: () => void;
    selectAgent: (values: ReadonlySet<AgentKind>) => void;
    selectTenant: (values: ReadonlySet<TenantSelectionValue>) => void;
    tenant: TenantSelection;
    tenantError: string | null;
    tenantOptions: SelectionOption<TenantSelectionValue>[];
  };
  detail: {
    closeConfigDetail: () => void;
    detailBackButtonRef: RefObject<HTMLButtonElement | null>;
    detailHeadingRef: RefObject<HTMLHeadingElement | null>;
    detailOpen: boolean;
    openConfig: (name: string) => void;
    openCurrent: () => void;
    selection: ConfigSelection;
  };
  selection: {
    allSelectable: boolean;
    cancelSelection: () => void;
    refreshButton: RefObject<HTMLButtonElement | null>;
    registerConfigRow: (key: string, element: HTMLButtonElement | null) => void;
    selectButton: RefObject<HTMLButtonElement | null>;
    selectableNames: string[];
    selectedCount: number;
    selectedKeys: Set<string>;
    selectionMode: boolean;
    enterSelection: () => void;
    toggleAllConfigs: () => void;
    toggleConfig: (name: string) => void;
  };
  mutations: {
    applyConfig: (name: string) => Promise<void>;
    busy: boolean;
    createConfig: (name: string) => Promise<void>;
    deleteConfigs: () => Promise<void>;
    executePropagation: () => Promise<void>;
    mutationBusy: boolean;
    previewPropagation: () => Promise<void>;
    requestDelete: (names: string[]) => void;
    saveAll: () => Promise<void>;
    saveInOrder: (names: readonly string[]) => Promise<boolean>;
    saveOrder: string[];
    savePending: (names: readonly string[]) => Promise<void>;
  };
  dialogs: {
    applyTarget: ConfigApplyTarget | null;
    cancelApply: () => void;
    cancelDelete: () => void;
    cancelPending: () => void;
    changeNewName: (name: string) => void;
    closeCreateDialog: () => void;
    closePropagation: () => void;
    createError: string | null;
    createHelpId: string;
    createNameTaken: boolean;
    createNameValid: boolean;
    createOpen: boolean;
    createTitleId: string;
    deleteTarget: ConfigDeleteTarget | null;
    discardAndRunPendingAction: () => Promise<void>;
    newName: string;
    openCreateDialog: () => void;
    pendingAction: ConfigPendingAction | null;
    preview: PropagationPreview | null;
    propagationHasFailures: boolean;
    propagationNeedsAttention: boolean;
    propagationTitleId: string;
    report: PropagationReport | null;
    requestApply: (name: string) => void;
    unsavedTitleId: string;
  };
  editor: {
    session: ConfigEditorSession;
    dirtyFiles: readonly string[];
    editorMode: "visual" | "raw";
    registerPane: (name: string, element: HTMLDivElement | null) => void;
    requestEditorAction: (
      action: () => void | Promise<void>,
      kind?: ConfigPendingAction["kind"],
    ) => void;
    retryReveals: () => void;
    showRawEditor: () => void;
    switchEditorMode: (next: "visual" | "raw") => void;
    visualAvailable: boolean;
  };
  feedback: {
    appliedName: string | null;
    applyFeedback: string | null;
    dismissNotification: (source: NotificationSource) => void;
    error: string | null;
    notifications: NotificationItemData[];
    setError: Dispatch<SetStateAction<string | null>>;
  };
}

export interface ConfigFileStatus {
  dirty: boolean;
  canSave: boolean;
}
