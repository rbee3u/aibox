import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError } from "./api";
import { bodyComplete, bodyHeaders, contentCoding, isSseResponse } from "./bodyPresentation";
import type {
  BodyKind,
  BodyLoadStatus,
  DecodedBodyState,
  DetailTab,
  EventTimingIndex,
  RecordDetail,
  RecordState,
  RecordSummary,
  TrafficApi,
} from "./types";
import { mergeEventTimings } from "./utils";

type InspectionErrorSource = "detail" | "body";
type InspectionErrors = Record<InspectionErrorSource, string | null>;

interface UseRecordInspectionOptions {
  api: TrafficApi;
  records: RecordSummary[];
}

const ACTIVE_DETAIL_POLL_INTERVAL_MS = 3000;
const EMPTY_DECODED_BODY: DecodedBodyState = { bytes: null, error: null };
const EMPTY_ERRORS: InspectionErrors = { detail: null, body: null };
const EMPTY_BODIES: Record<BodyKind, Uint8Array[]> = { request: [], response: [] };
const EMPTY_BODY_STATUS: Record<BodyKind, BodyLoadStatus> = {
  request: "idle",
  response: "idle",
};
const EMPTY_DECODED_BODIES: Record<BodyKind, DecodedBodyState> = {
  request: EMPTY_DECODED_BODY,
  response: EMPTY_DECODED_BODY,
};

export function useRecordInspection({ api, records }: UseRecordInspectionOptions) {
  const [currentId, setCurrentId] = useState<string | null>(null);
  const currentIdRef = useRef<string | null>(null);
  const [currentState, setCurrentState] = useState<RecordState | null>(null);
  const [detail, setDetail] = useState<RecordDetail | null>(null);
  const [bodies, setBodies] = useState(EMPTY_BODIES);
  const [bodyStatus, setBodyStatus] = useState(EMPTY_BODY_STATUS);
  const [decodedBodies, setDecodedBodies] = useState(EMPTY_DECODED_BODIES);
  const [eventTimings, setEventTimings] = useState<EventTimingIndex | null>(null);
  const timingNextSequence = useRef(0);
  const offsets = useRef({ request: 0, response: 0 });
  const decodedLoaded = useRef({ request: false, response: false });
  const [tab, setTab] = useState<DetailTab>("summary");
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [loadingBody, setLoadingBody] = useState(false);
  const [errors, setErrors] = useState<InspectionErrors>(EMPTY_ERRORS);
  const detailController = useRef<AbortController | null>(null);
  const bodyController = useRef<AbortController | null>(null);

  const reportError = useCallback((source: InspectionErrorSource, cause: unknown) => {
    const message = typeof cause === "string" ? cause : errorMessage(cause);
    setErrors((current) => ({ ...current, [source]: message }));
  }, []);
  const clearError = useCallback((source: InspectionErrorSource) => {
    setErrors((current) => (current[source] === null ? current : { ...current, [source]: null }));
  }, []);
  const resetRecordData = useCallback(() => {
    setDetail(null);
    setBodies(EMPTY_BODIES);
    setBodyStatus(EMPTY_BODY_STATUS);
    setDecodedBodies(EMPTY_DECODED_BODIES);
    setEventTimings(null);
    timingNextSequence.current = 0;
    decodedLoaded.current = { request: false, response: false };
    offsets.current = { request: 0, response: 0 };
    setTab("summary");
    setLoadingBody(false);
    setErrors((current) =>
      current.detail === null && current.body === null ? current : EMPTY_ERRORS,
    );
  }, []);
  const clearCurrentRecord = useCallback(() => {
    detailController.current?.abort();
    bodyController.current?.abort();
    detailController.current = null;
    bodyController.current = null;
    currentIdRef.current = null;
    setCurrentId(null);
    setCurrentState(null);
    setLoadingDetail(false);
    resetRecordData();
  }, [resetRecordData]);

  const selectRecord = useCallback(
    async (id: string) => {
      detailController.current?.abort();
      bodyController.current?.abort();
      const controller = new AbortController();
      detailController.current = controller;
      currentIdRef.current = id;
      setCurrentId(id);
      setCurrentState(records.find((record) => record.id === id)?.state ?? null);
      resetRecordData();
      setLoadingDetail(true);
      try {
        const record = await api.getRecord(id, controller.signal);
        if (detailController.current !== controller || controller.signal.aborted) return;
        setDetail(record);
        setCurrentState(record.state);
      } catch (cause) {
        if (
          detailController.current === controller &&
          !requestWasCancelled(cause, controller.signal)
        ) {
          if (isNotFound(cause)) clearCurrentRecord();
          reportError("detail", cause);
        }
      } finally {
        if (detailController.current === controller) setLoadingDetail(false);
      }
    },
    [api, clearCurrentRecord, records, reportError, resetRecordData],
  );

  const syncCurrentState = useCallback((freshRecords: RecordSummary[]) => {
    const selectedId = currentIdRef.current;
    const currentSummary = selectedId
      ? freshRecords.find((record) => record.id === selectedId)
      : undefined;
    if (currentSummary) {
      setCurrentState((current) =>
        current && current !== "active" ? current : currentSummary.state,
      );
    }
  }, []);

  const clearRecordIfCurrent = useCallback(
    (id: string) => {
      if (currentIdRef.current === id) clearCurrentRecord();
    },
    [clearCurrentRecord],
  );

  const activeId = detail?.state === "active" ? detail.request.id : null;

  useEffect(() => {
    if (loadingDetail || !activeId) return;
    const id = activeId;
    const controller = detailController.current;
    if (!controller) return;
    let disposed = false;
    let timer: number | undefined;
    let shouldContinue = true;
    const ownsRequest = () =>
      !disposed &&
      detailController.current === controller &&
      currentIdRef.current === id &&
      !controller.signal.aborted;
    const poll = async () => {
      if (!ownsRequest()) return;
      try {
        const fresh = await api.getRecord(id, controller.signal);
        if (!ownsRequest()) return;
        setDetail(fresh);
        setCurrentState(fresh.state);
        clearError("detail");
        shouldContinue = fresh.state === "active";
      } catch (cause) {
        if (ownsRequest() && !requestWasCancelled(cause, controller.signal)) {
          if (isNotFound(cause)) {
            shouldContinue = false;
            clearCurrentRecord();
          }
          reportError("detail", cause);
        }
      } finally {
        if (shouldContinue && ownsRequest()) {
          timer = window.setTimeout(() => void poll(), ACTIVE_DETAIL_POLL_INTERVAL_MS);
        }
      }
    };
    timer = window.setTimeout(() => void poll(), ACTIVE_DETAIL_POLL_INTERVAL_MS);
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [activeId, api, clearCurrentRecord, clearError, loadingDetail, reportError]);

  const detailState = detail?.state ?? null;
  const responseAvailable = Boolean(detail?.response);
  const visibleBodyKind = tab === "summary" ? null : tab;
  const visibleBodyCodingKind =
    detail && visibleBodyKind
      ? contentCoding(bodyHeaders(detail, visibleBodyKind)).kind
      : "identity";
  const visibleBodyComplete =
    detail !== null && visibleBodyKind !== null ? bodyComplete(detail, visibleBodyKind) : false;
  const shouldLoadVisibleTimings =
    visibleBodyKind === "response" && detail !== null && isSseResponse(detail);

  useEffect(() => {
    bodyController.current?.abort();
    if (loadingDetail || detailState === null || !currentId || tab === "summary") return;
    if (tab === "response" && !responseAvailable) return;

    const id = currentId;
    const kind = tab;
    const active = detailState === "active";
    const controller = new AbortController();
    bodyController.current = controller;
    let disposed = false;
    let timer: number | undefined;
    let shouldContinue = active && !visibleBodyComplete;
    const ownsRequest = () =>
      !disposed &&
      bodyController.current === controller &&
      currentIdRef.current === id &&
      !controller.signal.aborted;

    const poll = async () => {
      if (!ownsRequest()) return;
      setLoadingBody(true);
      try {
        const chunk = await api.loadBody(id, kind, offsets.current[kind], controller.signal);
        if (!ownsRequest()) return;
        if (chunk.bytes.length > 0) {
          setBodies((current) => ({ ...current, [kind]: [...current[kind], chunk.bytes] }));
        }
        offsets.current[kind] = chunk.nextOffset;
        setBodyStatus((current) => ({ ...current, [kind]: "loaded" }));
        if (
          visibleBodyCodingKind === "zstd" &&
          visibleBodyComplete &&
          !decodedLoaded.current[kind]
        ) {
          setDecodedBodies((current) =>
            current[kind].error === null ? current : { ...current, [kind]: EMPTY_DECODED_BODY },
          );
          try {
            const decoded = await api.loadDecodedBody(id, kind, controller.signal);
            if (!ownsRequest()) return;
            decodedLoaded.current[kind] = true;
            setDecodedBodies((current) => ({
              ...current,
              [kind]: { bytes: decoded, error: null },
            }));
          } catch (cause) {
            if (!ownsRequest() || requestWasCancelled(cause, controller.signal)) return;
            if (isNotFound(cause)) shouldContinue = false;
            decodedLoaded.current[kind] = false;
            setDecodedBodies((current) => ({
              ...current,
              [kind]: {
                bytes: null,
                error: `Decoded Source unavailable: ${errorMessage(cause)}`,
              },
            }));
          }
        }

        if (shouldLoadVisibleTimings) {
          try {
            const timing = await api.loadEventTimings(
              id,
              timingNextSequence.current,
              controller.signal,
            );
            if (!ownsRequest()) return;
            timingNextSequence.current = Math.max(timingNextSequence.current, timing.next_sequence);
            setEventTimings((current) => mergeEventTimings(current, timing));
          } catch (cause) {
            if (!ownsRequest() || requestWasCancelled(cause, controller.signal)) return;
            if (isNotFound(cause)) shouldContinue = false;
            setEventTimings((current) => ({
              state: "unavailable",
              events: current?.events ?? [],
              next_sequence: timingNextSequence.current,
              warning: `SSE Event timing unavailable: ${errorMessage(cause)}`,
            }));
          }
        }
        clearError("body");
      } catch (cause) {
        if (ownsRequest() && !requestWasCancelled(cause, controller.signal)) {
          if (isNotFound(cause)) shouldContinue = false;
          setBodyStatus((current) => ({
            ...current,
            [kind]: current[kind] === "loaded" ? "loaded" : "error",
          }));
          reportError("body", cause);
        }
      } finally {
        if (bodyController.current === controller) setLoadingBody(false);
        if (shouldContinue && ownsRequest()) {
          timer = window.setTimeout(() => void poll(), ACTIVE_DETAIL_POLL_INTERVAL_MS);
        }
      }
    };

    void poll();
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
      controller.abort();
      if (bodyController.current === controller) {
        bodyController.current = null;
        setLoadingBody(false);
      }
    };
  }, [
    api,
    clearError,
    currentId,
    detailState,
    loadingDetail,
    reportError,
    responseAvailable,
    shouldLoadVisibleTimings,
    tab,
    visibleBodyCodingKind,
    visibleBodyComplete,
  ]);

  useEffect(
    () => () => {
      detailController.current?.abort();
      bodyController.current?.abort();
    },
    [],
  );

  const download = useCallback(
    async (kind: BodyKind) => {
      const id = currentIdRef.current;
      const controller = detailController.current;
      if (!id || !controller) return;
      try {
        const { bytes: data } = await api.loadBody(id, kind, 0, controller.signal);
        if (controller.signal.aborted || detailController.current !== controller) return;
        const bodyBuffer = data.buffer.slice(
          data.byteOffset,
          data.byteOffset + data.byteLength,
        ) as ArrayBuffer;
        const url = URL.createObjectURL(new Blob([bodyBuffer]));
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = `${id}.${kind}.body`;
        anchor.click();
        window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      } catch (cause) {
        if (
          detailController.current === controller &&
          !requestWasCancelled(cause, controller.signal)
        ) {
          reportError("body", cause);
        }
      }
    },
    [api, reportError],
  );

  const error = errors.detail ?? errors.body;
  const errorSource = errors.detail ? "detail" : "body";
  const clearVisibleError = useCallback(() => clearError(errorSource), [clearError, errorSource]);

  return {
    bodies,
    bodyStatus,
    clearCurrentRecord,
    clearVisibleError,
    clearRecordIfCurrent,
    currentId,
    currentState,
    decodedBodies,
    detail,
    download,
    error,
    eventTimings,
    loadingBody,
    loadingDetail,
    selectRecord,
    setTab,
    syncCurrentState,
    tab,
  };
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : "Traffic management request failed";
}

function requestWasCancelled(cause: unknown, signal: AbortSignal): boolean {
  return (
    signal.aborted ||
    (typeof cause === "object" && cause !== null && "name" in cause && cause.name === "AbortError")
  );
}

function isNotFound(cause: unknown): cause is ApiError {
  return cause instanceof ApiError && cause.status === 404;
}
