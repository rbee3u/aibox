import type { RefObject } from "react";
import type { AgentKind } from "@/domain/agent";
import type { OverviewApi, OverviewData, TopologyData } from "@/api/overview";
import type { ConsoleNavigate, ModuleId } from "@/shared/lib/navigation";
import type { Operation } from "@/api/operations";
import type { OverviewBrowsingMemory } from "@/features/overview/browsingState";

export interface OverviewPageProps {
  browsingMemory?: OverviewBrowsingMemory;
  api: OverviewApi;
  operation: Operation | null;
  onNavigate: ConsoleNavigate;
  onOperation: (operation: Operation) => void;
}

/**
 * Types more than one Overview concern shares.
 *
 * Topology and Component facts share this feature-local tone vocabulary.
 * Other Console surfaces define tones for different UI semantics.
 */
export type Tone = "good" | "neutral" | "warning" | "error";
/**
 * What the attention panel is currently showing.
 *
 * The health logic in `topology/` decides it and the panel in `components/`
 * renders it, so the type belongs to neither concern.
 */
export type AttentionPanelKind = "items" | "pending" | "healthy";

export interface NavigationTarget {
  module: ModuleId;
  query?: URLSearchParams;
}
export interface AttentionItem {
  /** Stable identity: several items now share one category label. */
  key: string;
  label: string;
  detail: string;
  /**
   * The raw diagnostic behind `detail`, when the row states a cause the
   * Console worded itself. Kept as evidence behind a disclosure so the
   * sentence stays readable without discarding what the Service reported.
   */
  technical?: string;
  tone: "warning" | "error";
  target?: NavigationTarget;
  retry?: "service" | "topology";
}
/** One linked fact in the Tenant status table. */
export interface ResourceStatus {
  label: string;
  detail?: string;
  tone: Tone;
  target: NavigationTarget;
}

export interface AgentStatus {
  applicationLabel?: string;
  current: ResourceStatus;
  configs: ResourceStatus;
  sessions: ResourceStatus;
}

export interface TenantStatusRow extends ResourceStatus {
  id: string;
  kind: "host" | "managed";
  components: ResourceStatus;
  agents: Partial<Record<AgentKind, AgentStatus>>;
}

export interface OverviewViewModel {
  service: {
    build: (force: boolean) => Promise<void>;
    buildDisabled: boolean;
    buildUnavailableReason: string | null;
    elapsedUptime: number;
    loadOverview: (visibleRefresh?: boolean) => Promise<void>;
    overview: OverviewData | null;
    overviewError: string | null;
    overviewRefreshing: boolean;
    requestsTotal: number | null;
  };
  topology: {
    pageRef: RefObject<HTMLDivElement | null>;
    tenants: TenantStatusRow[] | null;
    loadTopology: (visibleRefresh?: boolean) => Promise<void>;
    topology: TopologyData | null;
    topologyError: string | null;
    topologyRefreshing: boolean;
  };
  attention: {
    attentionItems: AttentionItem[];
    panel: AttentionPanelKind;
  };
}
