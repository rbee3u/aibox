import type { RefObject } from "react";
import type { TenantRow } from "@/api/core";
import type {
  TenantApi,
  ComponentKind,
  ComponentLatestSnapshot,
  ComponentRow,
} from "@/api/tenants";

import type { ComponentGroup } from "@/features/tenants/components/componentCatalog";

import type { TenantDeleteTarget } from "@/features/tenants/tenantWorkflow";
import type { NotificationItemData, NotificationSource } from "@/shared/ui/notificationTypes";
import type { TenantSelectionValue } from "@/domain/tenant";
import type { Operation } from "@/api/operations";
import type { ModuleLocationChange } from "@/shared/lib/navigation";

export interface TenantPageProps {
  api: TenantApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}

export interface TenantViewModel {
  catalog: {
    hostTenant: TenantRow | null;
    loadingTenants: boolean;
    managedTenants: Array<TenantRow & { kind: "managed"; name: string }>;
    refreshButton: RefObject<HTMLButtonElement | null>;
    refreshing: boolean;
    refreshTenants: () => Promise<void>;
    retryTenantPage: () => Promise<void>;
    selectButton: RefObject<HTMLButtonElement | null>;
    tenantCatalogError: string | null;
  };
  detail: {
    copiedHome: string | null;
    copyHome: (text: string, value: string) => Promise<void>;
    detailHeadingRef: RefObject<HTMLHeadingElement | null>;
    detailOpen: boolean;
    selected: TenantRow | null;
    selectedHome: string;
    selectedKey: TenantSelectionValue | null;
    tenantKindLabel: string;
  };
  selection: {
    allSelectable: boolean;
    cancelSelection: () => void;
    selectedCount: number;
    selectedKeys: Set<TenantSelectionValue>;
    selectableKeys: TenantSelectionValue[];
    selectionMode: boolean;
    enterSelection: () => void;
    focusTenantRow: (key: TenantSelectionValue) => void;
    registerTenantRow: (key: TenantSelectionValue, element: HTMLButtonElement | null) => void;
    toggleAllTenants: () => void;
    toggleTenant: (key: TenantSelectionValue) => void;
  };
  components: {
    allComponents: ComponentRow[];
    checkingLatest: boolean;
    checkForUpdates: () => Promise<void>;
    closeComponentMenu: () => void;
    componentActionProgress: ComponentActionProgress | null;
    componentCatalogLoading: boolean;
    componentGroups: Array<ComponentGroup & { rows: ComponentRow[] }>;
    componentMenuPosition: { top: number; left: number } | null;
    componentMenuRef: RefObject<HTMLDivElement | null>;
    componentTotalCount: number;
    differingComponentCount: number;
    installedComponentCount: number;
    /** Installs, repairs, or updates the row; an overwriting Update confirms first. */
    installComponent: (row: ComponentRow, requestedVersion?: string | null) => void;
    issueComponentCount: number;
    outdatedComponentCount: number;
    latestSnapshot: ComponentLatestSnapshot | null;
    loadComponents: (target: TenantRow | null, showLoading?: boolean) => Promise<void>;
    attentionKind: ComponentKind | null;
    openComponentMenu: (kind: ComponentKind, anchor: HTMLElement, width: number) => void;
    openMenu: ComponentKind | null;
    openSpecificVersion: (row: ComponentRow, mode: ComponentSpecificVersionTarget["mode"]) => void;
    registerComponentMenuButton: (kind: ComponentKind, element: HTMLButtonElement | null) => void;
    registerComponentMenuItem: (kind: ComponentKind, element: HTMLButtonElement | null) => void;
    submitSpecificVersion: () => Promise<void>;
    toggleComponentMenu: (kind: ComponentKind, anchor: HTMLElement, width: number) => void;
  };
  mutations: {
    busy: boolean;
    createTenant: () => Promise<void>;
    deleteTenants: () => Promise<void>;
    mutationBusy: boolean;
    requestTenantDelete: (names: string[]) => void;
  };
  dialogs: {
    cancelComponentRemove: () => void;
    cancelComponentUpdate: () => void;
    cancelDeleteDialog: () => void;
    changeNewName: (name: string) => void;
    changeSpecificVersion: (value: string) => void;
    closeCreateDialog: () => void;
    closeSpecificVersion: () => void;
    componentRemoveTarget: ComponentRemoveTarget | null;
    componentUpdateTarget: ComponentUpdateTarget | null;
    createError: string | null;
    createHelpId: string;
    createNameTaken: boolean;
    createNameValid: boolean;
    createOpen: boolean;
    createTitleId: string;
    deleteTarget: TenantDeleteTarget | null;
    newName: string;
    openCreateDialog: () => void;
    removeComponent: () => Promise<void>;
    requestComponentRemove: (row: ComponentRow, tenantLabel: string) => void;
    specificVersion: string;
    specificVersionError: string | null;
    specificVersionHelpId: string;
    specificVersionTarget: ComponentSpecificVersionTarget | null;
    specificVersionTitleId: string;
    specificVersionValid: boolean;
    specificVersionValidationError: string | null;
    updateComponent: () => Promise<void>;
  };
  feedback: {
    dismissNotification: (source: NotificationSource) => void;
    notifications: NotificationItemData[];
    error: string | null;
  };
}

export type ComponentRemoveTarget = { row: ComponentRow; tenantLabel: string };

export type ComponentUpdateTarget = { row: ComponentRow; tenantLabel: string };

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
