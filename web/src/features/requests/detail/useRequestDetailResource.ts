import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, type RequestDetail, type RequestLookup, type RequestsApi } from "@/api/requests";
import type {
  ClearInspectionFailure,
  ReportInspectionFailure,
  RequestInspectionIdentity,
} from "@/features/requests/detail/inspectionTypes";
import { requestErrorMessage, requestWasCancelled } from "@/features/requests/requestErrors";
import type { DetailTab } from "@/features/requests/viewTypes";

const ACTIVE_DETAIL_POLL_INTERVAL_MS = 3000;

interface DetailResourceOptions {
  api: RequestsApi;
  clearFailure: ClearInspectionFailure;
  initialTab: DetailTab;
  paused: boolean;
  reportFailure: ReportInspectionFailure;
}

export function useRequestDetailResource({
  api,
  clearFailure,
  initialTab,
  paused,
  reportFailure,
}: DetailResourceOptions) {
  const [identity, setIdentity] = useState<RequestInspectionIdentity | null>(null);
  const identityRef = useRef<RequestInspectionIdentity | null>(null);
  const nextGeneration = useRef(0);
  const [detail, setDetail] = useState<RequestDetail | null>(null);
  const [tab, setTab] = useState<DetailTab>(initialTab);
  const [loading, setLoading] = useState(false);
  const controllerRef = useRef<AbortController | null>(null);
  const resumeAfterPause = useRef(false);
  const refreshActiveAfterPause = useRef(false);

  const reset = useCallback((nextTab: DetailTab = "summary") => {
    setDetail(null);
    setTab(nextTab);
  }, []);

  const clear = useCallback(() => {
    controllerRef.current?.abort();
    controllerRef.current = null;
    identityRef.current = null;
    nextGeneration.current += 1;
    resumeAfterPause.current = false;
    refreshActiveAfterPause.current = false;
    setIdentity(null);
    setLoading(false);
    reset();
  }, [reset]);

  const select = useCallback(
    async (id: string, nextTab: DetailTab = "summary") => {
      controllerRef.current?.abort();
      const controller = new AbortController();
      const selected = { id, generation: ++nextGeneration.current };
      controllerRef.current = controller;
      identityRef.current = selected;
      setIdentity(selected);
      reset(nextTab);
      clearFailure();
      setLoading(true);
      const ownsRequest = () =>
        controllerRef.current === controller &&
        identityRef.current?.generation === selected.generation &&
        !controller.signal.aborted;
      try {
        const lookup = await api.getRequest(id, controller.signal);
        if (!ownsRequest()) return;
        if (isMissing(lookup)) {
          clear();
          reportFailure({ kind: "detail", message: "Request not found", retryable: false });
          return;
        }
        setDetail(lookup);
        clearFailure("detail");
      } catch (cause) {
        if (ownsRequest() && !requestWasCancelled(cause, controller.signal)) {
          const notFound = isNotFound(cause);
          if (notFound) clear();
          reportFailure({
            kind: "detail",
            message: requestErrorMessage(cause),
            retryable: !notFound,
          });
        }
      } finally {
        if (controllerRef.current === controller) setLoading(false);
      }
    },
    [api, clear, clearFailure, reportFailure, reset],
  );

  useEffect(() => {
    if (!paused) return;
    resumeAfterPause.current =
      resumeAfterPause.current || (identityRef.current !== null && detail === null);
    refreshActiveAfterPause.current = refreshActiveAfterPause.current || detail?.state === "active";
    controllerRef.current?.abort();
  }, [detail, paused]);

  useEffect(() => {
    if (paused || !resumeAfterPause.current) return;
    const selected = identityRef.current;
    resumeAfterPause.current = false;
    if (selected) void select(selected.id, tab);
  }, [paused, select, tab]);

  const activeIdentity = detail?.state === "active" ? identity : null;

  useEffect(() => {
    if (paused || loading || !activeIdentity) return;
    const selected = activeIdentity;
    const controller = controllerRef.current?.signal.aborted
      ? new AbortController()
      : (controllerRef.current ?? new AbortController());
    controllerRef.current = controller;
    let disposed = false;
    let timer: number | undefined;
    let shouldContinue = true;
    const ownsRequest = () =>
      !disposed &&
      controllerRef.current === controller &&
      identityRef.current?.generation === selected.generation &&
      !controller.signal.aborted;
    const poll = async () => {
      if (!ownsRequest()) return;
      try {
        const lookup = await api.getRequest(selected.id, controller.signal);
        if (!ownsRequest()) return;
        if (isMissing(lookup)) {
          shouldContinue = false;
          clear();
          reportFailure({ kind: "detail", message: "Request not found", retryable: false });
          return;
        }
        setDetail(lookup);
        clearFailure("detail");
        shouldContinue = lookup.state === "active";
      } catch (cause) {
        if (ownsRequest() && !requestWasCancelled(cause, controller.signal)) {
          const notFound = isNotFound(cause);
          if (notFound) {
            shouldContinue = false;
            clear();
          }
          reportFailure({
            kind: "detail",
            message: requestErrorMessage(cause),
            retryable: !notFound,
          });
        }
      } finally {
        if (shouldContinue && ownsRequest()) {
          timer = window.setTimeout(() => void poll(), ACTIVE_DETAIL_POLL_INTERVAL_MS);
        }
      }
    };
    if (refreshActiveAfterPause.current) {
      refreshActiveAfterPause.current = false;
      void poll();
    } else {
      timer = window.setTimeout(() => void poll(), ACTIVE_DETAIL_POLL_INTERVAL_MS);
    }
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [activeIdentity, api, clear, clearFailure, loading, paused, reportFailure]);

  useEffect(() => () => controllerRef.current?.abort(), []);

  return {
    clear,
    currentId: identity?.id ?? null,
    detail,
    identity,
    loading,
    select,
    setTab,
    tab,
  };
}

function isNotFound(cause: unknown): cause is ApiError {
  return cause instanceof ApiError && cause.status === 404;
}

function isMissing(value: RequestLookup): value is { kind: "missing" } {
  return typeof value === "object" && value !== null && "kind" in value && value.kind === "missing";
}
