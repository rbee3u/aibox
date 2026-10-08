import type { RefObject } from "react";

import type {
  RequestsApi,
  RequestDetail,
  EventTimingIndex,
  RequestList,
  BodyKind,
} from "@/api/requests";
import type { RequestsDialog } from "@/features/requests/requestsWorkflow";
import type { NotificationItemData, NotificationSource } from "@/shared/ui/notificationTypes";
import type { ModuleLocationChange } from "@/shared/lib/navigation";

export interface RequestsPageProps {
  api: RequestsApi;
  search: string;
  onLocationChange: ModuleLocationChange;
}

export type BodyLoadStatus = "idle" | "loaded" | "error";
export type DetailTab = "summary" | BodyKind;

export interface DecodedBodyState {
  bytes: Uint8Array | null;
  error: string | null;
}

export interface RequestsViewModel {
  catalog: {
    currentId: string | null;
    list: RequestList;
    listError: string | null;
    loadingList: boolean;
    navigatePage: (nextPage: number) => void;
    openRequest: (id: string) => void;
    page: number;
    refreshPage: () => Promise<void>;
    refreshing: boolean;
    /** Absent in selection mode: reloading the list would discard the selection. */
    retryList: (() => void) | undefined;
  };
  detail: {
    bodies: Record<BodyKind, Uint8Array[]>;
    bodyStatus: Record<BodyKind, BodyLoadStatus>;
    currentId: string | null;
    decodedBodies: Record<BodyKind, DecodedBodyState>;
    detail: RequestDetail | null;
    detailBackButton: RefObject<HTMLButtonElement | null>;
    detailOpen: boolean;
    download: (kind: BodyKind) => Promise<void>;
    eventTimings: EventTimingIndex | null;
    inspectionFailure: InspectionFailure | null;
    loadingBody: boolean;
    loadingDetail: boolean;
    retryInspectionFailure: () => void;
    returnToList: () => void;
    selectTab: (next: DetailTab) => void;
    tab: DetailTab;
  };
  selection: {
    clearFocusAfterDelete: () => void;
    clearFocusAfterInspection: () => void;
    enterSelection: () => void;
    exitSelection: () => void;
    focusAfterDelete: string | null | undefined;
    focusAfterInspection: string | null | undefined;
    selected: Set<string>;
    selectionMode: boolean;
    togglePageSelection: () => void;
    toggleRequestSelection: (id: string) => void;
  };
  mutations: {
    deletingRequestId: string | null;
    deletionBusy: boolean;
    openBatchDeletion: () => void;
    openRequestDeletion: (id: string) => void;
  };
  dialogs: {
    cancelDialog: () => void;
    confirmDelete: () => Promise<void>;
    dialog: RequestsDialog;
  };
  feedback: {
    dismissNotification: (source: NotificationSource) => void;
    handleNotificationAction: (notification: NotificationItemData) => void;
    notifications: NotificationItemData[];
  };
}

export interface RequestInspectionIdentity {
  id: string;
  generation: number;
}

export interface InspectionFailure {
  kind: "detail" | "body" | "download";
  message: string;
  bodyKind?: BodyKind;
  retryable?: boolean;
}

export type ReportInspectionFailure = (failure: InspectionFailure) => void;

export type ClearInspectionFailure = (kind?: InspectionFailure["kind"]) => void;
