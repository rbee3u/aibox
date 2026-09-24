import type { RefObject } from "react";
import { useEffect, useRef, useState } from "react";

import type { Operation } from "@/api/operations";
import type { SessionApi } from "@/api/sessions";
import type { AgentKind } from "@/domain/agent";
import type { TenantSelection } from "@/domain/tenant";
import {
  focusTargetAfterSessionDelete,
  type AggregatedSessionData,
  type SourcedSession,
} from "@/features/sessions/sessionSource";
import { useElementRegistry } from "@/features/common/useElementRegistry";

export type SessionDeletion = { kind: "record"; key: string } | { kind: "batch" } | null;
type SessionDeleteDialog =
  { kind: "record"; target: SourcedSession } | { kind: "batch"; keys: string[] } | null;

interface SessionDeletionOptions {
  abortDetailStream: () => void;
  api: Pick<SessionApi, "deleteSessions">;
  clearInspection: () => void;
  data: AggregatedSessionData | null;
  inspectedSession: () => SourcedSession | null;
  listUnavailable: boolean;
  load: (kind?: "initial" | "refresh") => Promise<AggregatedSessionData | null>;
  openSession: (
    row: SourcedSession,
    updateLocation?: boolean,
    preserveContent?: boolean,
  ) => Promise<void>;
  operation?: Operation | null;
  onSelectionRecovery: (remaining: Set<string>) => void;
  refreshButton: RefObject<HTMLButtonElement | null>;
  removeSession: (key: string) => void;
  reportFailure: (source: "action", title: string, cause: unknown) => void;
  resolveFailure: (source: "action") => void;
  tenant: TenantSelection;
  agent: AgentKind;
}

/** Owns Session deletion requests, dialog state, partial failure recovery, and focus restoration. */
export function useSessionDeletion({
  abortDetailStream,
  api,
  clearInspection,
  data,
  inspectedSession,
  listUnavailable,
  load,
  openSession,
  operation,
  onSelectionRecovery,
  refreshButton,
  removeSession,
  reportFailure,
  resolveFailure,
  tenant,
  agent,
}: SessionDeletionOptions) {
  const [dialog, setDialog] = useState<SessionDeleteDialog>(null);
  const [deletion, setDeletion] = useState<SessionDeletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const deletionInFlight = useRef(false);
  const deleteButtons = useElementRegistry<HTMLButtonElement>();
  const dialogKeys = dialog?.kind === "batch" ? dialog.keys : null;
  const singleDeleteTarget = dialog?.kind === "record" ? dialog.target : null;

  useEffect(() => {
    // A filter change invalidates any pending deletion target and focus plan.
    /* eslint-disable react-hooks/set-state-in-effect */
    setDialog(null);
    setFocusAfterDelete(undefined);
    /* eslint-enable react-hooks/set-state-in-effect */
  }, [agent, tenant]);

  useEffect(() => {
    if (focusAfterDelete === undefined || deletion !== null) return;
    const preferred = focusAfterDelete ? deleteButtons.get(focusAfterDelete) : null;
    const target = preferred && !preferred.disabled ? preferred : refreshButton.current;
    if (target && !target.disabled) {
      target.focus();
      // The focus target is consumed once; clearing it here is what ends the
      // post-deletion focus move rather than a cascading state update.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setFocusAfterDelete(undefined);
    }
  }, [data, deleteButtons, deletion, focusAfterDelete, refreshButton]);

  function setDialogKeys(keys: string[] | null) {
    setDialog(keys ? { kind: "batch", keys } : null);
  }

  function setSingleDeleteTarget(target: SourcedSession | null) {
    setDialog(target ? { kind: "record", target } : null);
  }

  function registerDeleteButton(key: string, element: HTMLButtonElement | null) {
    deleteButtons.register(key, element);
  }

  function beginDeletion(next: Exclude<SessionDeletion, null>): boolean {
    if (deletionInFlight.current) return false;
    deletionInFlight.current = true;
    setDeletion(next);
    return true;
  }

  function finishDeletion() {
    deletionInFlight.current = false;
    setDeletion(null);
  }

  async function deleteSession(row: SourcedSession) {
    if (
      operation?.state === "running" ||
      data?.warnings.length ||
      listUnavailable ||
      !data ||
      !beginDeletion({ kind: "record", key: row.key })
    )
      return;
    const originRows = data.sessions;
    const wasCurrent = inspectedSession()?.key === row.key;
    if (wasCurrent) abortDetailStream();
    resolveFailure("action");
    try {
      await api.deleteSessions(row.source.tenant, row.source.agent, [row.id]);
      removeSession(row.key);
      if (wasCurrent) clearInspection();
      await load("refresh");
      setFocusAfterDelete(focusTargetAfterSessionDelete(originRows, row.key));
    } catch (cause) {
      reportFailure("action", "Couldn’t delete Session", cause);
      const refreshed = await load("refresh");
      const survivor = refreshed?.sessions.find((session) => session.key === row.key);
      if (wasCurrent && survivor) void openSession(survivor);
      setFocusAfterDelete(survivor ? row.key : null);
    } finally {
      setSingleDeleteTarget(null);
      finishDeletion();
    }
  }

  async function deleteSelectedSessions() {
    if (
      operation?.state === "running" ||
      !dialogKeys ||
      dialogKeys.length === 0 ||
      !beginDeletion({ kind: "batch" })
    )
      return;
    const keys = dialogKeys;
    const keySet = new Set(keys);
    const selectedRows = data?.sessions.filter((row) => keySet.has(row.key)) ?? [];
    const ids = selectedRows.map((row) => row.id);
    const currentKey = inspectedSession()?.key;
    const wasCurrent = currentKey ? keySet.has(currentKey) : false;
    if (wasCurrent) clearInspection();
    resolveFailure("action");
    try {
      await api.deleteSessions(tenant, agent, ids);
    } catch (cause) {
      reportFailure("action", "Couldn’t delete selected Sessions", cause);
    }
    setDialogKeys(null);
    const refreshed = await load("refresh");
    if (refreshed && refreshed.warnings.length === 0) {
      const remaining = new Set(
        keys.filter((key) => refreshed.sessions.some((row) => row.key === key)),
      );
      onSelectionRecovery(remaining);
      if (wasCurrent && currentKey) {
        const survivor = refreshed.sessions.find((row) => row.key === currentKey);
        if (survivor) void openSession(survivor);
      }
    }
    setFocusAfterDelete(null);
    finishDeletion();
  }

  const deletionBusy = deletion !== null;

  // Grouped the way the Session view model consumes it, so the controller
  // spreads these rather than forwarding each field.
  return {
    mutations: {
      batchBusy: deletion?.kind === "batch",
      deleteSelectedSessions,
      deleteSession,
      deletion,
      deletionBusy,
      mutationBusy: deletionBusy || operation?.state === "running",
    },
    dialogs: {
      dialogKeys,
      closeBatchDelete: () => setDialogKeys(null),
      closeSingleDelete: () => setSingleDeleteTarget(null),
      openBatchDelete: (keys: string[]) => setDialogKeys(keys),
      openSingleDelete: (target: SourcedSession) => setSingleDeleteTarget(target),
      registerDeleteButton,
      singleDeleteTarget,
    },
  };
}
