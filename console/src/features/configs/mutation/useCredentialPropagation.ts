import { useState } from "react";

import type {
  ConfigApi,
  ConfigListData,
  PropagationPreview,
  PropagationReport,
} from "@/api/configs";
import { propagationGroup } from "@/features/configs/configCatalog";
import type { ConfigCatalogLoadKind } from "@/features/configs/viewTypes";

interface CredentialPropagationOptions {
  api: Pick<ConfigApi, "executeCredentialPropagation" | "previewCredentialPropagation">;
  loadCatalog: (kind?: ConfigCatalogLoadKind) => Promise<ConfigListData | null>;
  onBusyChange: (busy: boolean) => void;
  operationRunning: boolean;
  reportActionFailure: (title: string, cause: unknown) => void;
}

export function useCredentialPropagation({
  api,
  loadCatalog,
  onBusyChange,
  operationRunning,
  reportActionFailure,
}: CredentialPropagationOptions) {
  const [preview, setPreview] = useState<PropagationPreview | null>(null);
  const [report, setReport] = useState<PropagationReport | null>(null);

  async function previewPropagation() {
    onBusyChange(true);
    try {
      setPreview(await api.previewCredentialPropagation());
      setReport(null);
    } catch (cause) {
      reportActionFailure("Couldn’t preview credential propagation", cause);
    } finally {
      onBusyChange(false);
    }
  }

  async function executePropagation() {
    if (operationRunning || !preview) return;
    onBusyChange(true);
    try {
      setReport(await api.executeCredentialPropagation(preview.plan_id));
      setPreview(null);
      await loadCatalog("background");
    } catch (cause) {
      reportActionFailure("Couldn’t propagate credentials", cause);
    } finally {
      onBusyChange(false);
    }
  }

  function closePropagation() {
    setPreview(null);
    setReport(null);
  }

  const propagationHasFailures =
    report?.entries.some((entry) => entry.outcome.status === "failed") ?? false;
  const propagationNeedsAttention =
    report?.entries.some((entry) => propagationGroup(entry.outcome.status) === "attention") ??
    false;

  return {
    mutations: {
      executePropagation,
      previewPropagation,
    },
    dialogs: {
      closePropagation,
      preview,
      propagationHasFailures,
      propagationNeedsAttention,
      report,
    },
  };
}
