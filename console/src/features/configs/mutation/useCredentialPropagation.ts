import type { Dispatch, SetStateAction } from "react";
import { useState } from "react";

import type {
  ConfigApi,
  ConfigListData,
  PropagationPreview,
  PropagationReport,
} from "@/api/configs";
import { propagationGroup } from "@/features/configs/configCatalog";
import type { ConfigCatalogLoadKind } from "@/features/configs/viewTypes";
import { messageOf } from "@/shared/lib/errors";

interface CredentialPropagationOptions {
  api: Pick<ConfigApi, "executeCredentialPropagation" | "previewCredentialPropagation">;
  loadCatalog: (kind?: ConfigCatalogLoadKind) => Promise<ConfigListData | null>;
  onBusyChange: (busy: boolean) => void;
  operationRunning: boolean;
  setError: Dispatch<SetStateAction<string | null>>;
}

export function useCredentialPropagation({
  api,
  loadCatalog,
  onBusyChange,
  operationRunning,
  setError,
}: CredentialPropagationOptions) {
  const [preview, setPreview] = useState<PropagationPreview | null>(null);
  const [report, setReport] = useState<PropagationReport | null>(null);

  async function previewPropagation() {
    onBusyChange(true);
    try {
      setPreview(await api.previewCredentialPropagation());
      setReport(null);
    } catch (cause) {
      setError(messageOf(cause));
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
      setError(messageOf(cause));
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
