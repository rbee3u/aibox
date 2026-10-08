import { tenantLocation } from "@/features/common/routes/tenants";
import { useEffect, useMemo, useRef, useState } from "react";

import { tenantStatusRows } from "@/features/overview/tenantStatus";
import {
  attentionPanelKind,
  buildDisabledReason,
  bySeverity,
  topologyAttentions,
} from "@/features/overview/topology/healthAttention";
import { explainFailure } from "@/features/overview/failureCopy";
import type {
  OverviewPageProps,
  AttentionItem,
  OverviewViewModel,
} from "@/features/overview/viewTypes";
import { useOverviewData } from "@/features/overview/useOverviewData";
import { messageOf } from "@/shared/lib/errors";

export function useOverviewController({
  api,
  operation,
  onOperation,
}: Pick<OverviewPageProps, "api" | "operation" | "onOperation">): OverviewViewModel {
  const [buildPosting, setBuildPosting] = useState(false);
  const ownedBuild = useRef<string | null>(null);
  const [buildError, setBuildError] = useState<string | null>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const {
    elapsedUptime,
    loadOverview,
    loadTopology,
    requestsTotal,
    overview,
    overviewError,
    overviewRefreshing,
    topology,
    topologyError,
    topologyRefreshing,
  } = useOverviewData(api);
  useEffect(() => {
    if (
      !ownedBuild.current ||
      operation?.id !== ownedBuild.current ||
      operation.state === "running"
    )
      return;
    ownedBuild.current = null;
    void loadOverview();
  }, [loadOverview, operation]);
  const tenants = useMemo(() => (topology ? tenantStatusRows(topology) : null), [topology]);
  const operationRunning = operation?.state === "running";
  const buildDisabled =
    buildPosting ||
    operationRunning ||
    overview?.docker.status !== "available" ||
    overviewError !== null;
  const buildUnavailableReason = buildPosting
    ? "Build request is being submitted."
    : buildDisabled
      ? overviewError
        ? "Refresh Service status before building."
        : buildDisabledReason(overview, operation)
      : null;
  const attentionItems = useMemo<AttentionItem[]>(() => {
    const items: AttentionItem[] = [];
    if (buildError)
      items.push({
        key: "build",
        label: "Runtime Image build",
        ...explainFailure("build", buildError),
        tone: "error",
      });
    if (overviewError)
      items.push({
        key: "service",
        label: "Service status",
        ...explainFailure("service", overviewError),
        tone: "error",
        retry: "service",
      });
    if (!overviewError && overview?.docker.status === "unavailable")
      items.push({
        key: "docker",
        label: "Docker",
        ...explainFailure("docker", overview.docker.error),
        tone: "error",
      });
    if (
      !overviewError &&
      overview?.docker.status === "available" &&
      overview.runtime_image.status !== "built"
    )
      items.push({
        key: "runtime-image",
        label: "Runtime Image",
        detail: overview.runtime_image.detail ?? "Build the Runtime Image before starting a Run.",
        tone: overview.runtime_image.status === "missing" ? "warning" : "error",
      });
    if (overview?.host_available === false)
      items.push({
        key: "host-tenant",
        label: "Host Tenant",
        detail: "The Host Home is unavailable.",
        tone: "warning",
        target: { module: "tenants", query: tenantLocation("host") },
      });
    if (topology) items.push(...topologyAttentions(topology));
    if (topologyError)
      items.push({
        key: "topology",
        label: "Resource inspection",
        ...explainFailure("topology", topologyError),
        tone: "error",
        retry: "topology",
      });
    return bySeverity(items);
  }, [buildError, overview, overviewError, topology, topologyError]);
  const panel = attentionPanelKind({
    itemCount: attentionItems.length,
    overviewSettled: (overview !== null || overviewError !== null) && !overviewRefreshing,
    topologySettled: (topology !== null || topologyError !== null) && !topologyRefreshing,
  });

  async function build(force: boolean) {
    setBuildPosting(true);
    setBuildError(null);
    try {
      const value = await api.buildImage(force);
      ownedBuild.current = value.id;
      onOperation(value);
    } catch (cause) {
      setBuildError(messageOf(cause));
    } finally {
      setBuildPosting(false);
    }
  }

  return {
    service: {
      build,
      buildDisabled,
      buildUnavailableReason,
      elapsedUptime,
      loadOverview,
      overview,
      overviewError,
      overviewRefreshing,
      requestsTotal,
    },
    topology: {
      pageRef,
      tenants,
      loadTopology,
      topology,
      topologyError,
      topologyRefreshing,
    },
    attention: {
      attentionItems,
      panel,
    },
  };
}
