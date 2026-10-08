import type { RefObject, UIEvent } from "react";

import type {
  SessionApi,
  ConversationMessage,
  SessionDetailMeta,
  SessionDetailStats,
  ToolActivity,
  TranscriptEvidenceSummary,
} from "@/api/sessions";
import type { AgentKind } from "@/domain/agent";
import type { TenantSelection, TenantSelectionValue } from "@/domain/tenant";
import type { SessionTab } from "@/features/common/routes/sessions";
import type { AggregatedSessionData, SourcedSession } from "@/features/sessions/sessionSource";
import type { SelectionOption } from "@/shared/ui/SelectionMenu";
import type { NotificationItemData, NotificationSource } from "@/shared/ui/notificationTypes";
import type { Operation } from "@/api/operations";
import type { ModuleLocationChange } from "@/shared/lib/navigation";

export interface SessionPageProps {
  api: SessionApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
}

export interface SessionViewModel {
  catalog: {
    agent: AgentKind;
    agentOptions: SelectionOption<AgentKind>[];
    data: AggregatedSessionData | null;
    load: (kind?: "initial" | "refresh") => Promise<AggregatedSessionData | null>;
    loadingList: boolean;
    loadingTenants: boolean;
    refreshButton: RefObject<HTMLButtonElement | null>;
    refreshing: boolean;
    retryPageError: () => void;
    retryTenants: () => void;
    selectAgent: (values: ReadonlySet<AgentKind>) => void;
    selectTenant: (values: ReadonlySet<TenantSelectionValue>) => void;
    sessions: SourcedSession[];
    sessionTenantMissing: boolean;
    tenant: TenantSelection;
    tenantError: string | null;
    tenantOptions: SelectionOption<TenantSelectionValue>[];
  };
  detail: {
    closeSessionInspection: () => void;
    conversationScrollRef: RefObject<HTMLDivElement | null>;
    currentSession: SourcedSession | null;
    detailHeadingRef: RefObject<HTMLHeadingElement | null>;
    detailMeta: SessionDetailMeta | null;
    detailRevision: number;
    detailStats: SessionDetailStats | null;
    jumpToLatest: () => void;
    jumpToUserMessage: (entryId: string) => void;
    loadingDetail: boolean;
    onConversationScroll: (event: UIEvent<HTMLDivElement>) => void;
    openSession: (
      row: SourcedSession,
      updateLocation?: boolean,
      preserveContent?: boolean,
    ) => Promise<void>;
    /**
     * Re-reads the inspected Session in place, as Refresh does, and resolves
     * to the snapshot the new read reported — `null` if it did not complete.
     */
    refreshTranscript: () => Promise<string | null>;
    resolvedActiveUserMessage: string | null;
    sessionTab: SessionTab;
    sessionWarnings: string[];
    showJumpLatest: boolean;
    timeline: SessionTimelineItem[];
    transcriptHasDiagnostics: boolean;
    /** Why Conversation reading is impaired; `null` when the Transcript reads cleanly. */
    transcriptAttentionNotice: string | null;
    transcriptIsPartial: boolean;
    unsafeView: boolean;
    updateSessionTab: (next: SessionTab) => void;
    registerUserMessage: (entryId: string, element: HTMLElement | null) => void;
    userMessages: ConversationMessage[];
  };
  selection: {
    allSelected: boolean;
    cancelSelection: () => void;
    selectedKeys: Set<string>;
    selectionMode: boolean;
    registerSessionRow: (key: string, element: HTMLButtonElement | null) => void;
    enterSelection: () => void;
    toggleAllSessions: () => void;
    toggleSession: (key: string) => void;
  };
  mutations: {
    batchBusy: boolean;
    deleteSelectedSessions: () => Promise<void>;
    deleteSession: (row: SourcedSession) => Promise<void>;
    deletion: SessionDeletion;
    deletionBusy: boolean;
    mutationBusy: boolean;
  };
  dialogs: {
    registerDeleteButton: (key: string, element: HTMLButtonElement | null) => void;
    dialogKeys: string[] | null;
    selectButton: RefObject<HTMLButtonElement | null>;
    closeBatchDelete: () => void;
    closeSingleDelete: () => void;
    openBatchDelete: (keys: string[]) => void;
    openSingleDelete: (target: SourcedSession) => void;
    singleDeleteTarget: SourcedSession | null;
  };
  feedback: {
    dismissNotification: (source: NotificationSource) => void;
    error: string | null;
    notifications: NotificationItemData[];
  };
}

export type SessionDeletion = { kind: "record"; key: string } | { kind: "batch" } | null;

export type SessionTimelineItem =
  | { kind: "message"; value: ConversationMessage }
  | { kind: "activity"; value: SessionActivityItem[] };

export type SessionActivityItem =
  | {
      kind: "tool";
      value: ToolActivity;
      /** The terminal record for this call, once it has arrived: what came back. */
      result?: ToolActivity;
    }
  | { kind: "evidence"; value: TranscriptEvidenceSummary };
