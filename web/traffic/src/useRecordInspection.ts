import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, requestErrorMessage, requestWasCancelled } from "./api";
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

export interface InspectionFailure {
  kind: "detail" | "body" | "download";
  message: string;
  bodyKind?: BodyKind;
  retryable?: boolean;
}

interface UseRecordInspectionOptions {
  api: TrafficApi;
  records: RecordSummary[];
  paused: boolean;
  onFailure: (failure: InspectionFailure) => void;
  onRecovery: () => void;
}

const ACTIVE_DETAIL_POLL_INTERVAL_MS = 3000;
const EMPTY_DECODED_BODY: DecodedBodyState = { bytes: null, error: null };
const EMPTY_BODIES: Record<BodyKind, Uint8Array[]> = { request: [], response: [] };
const EMPTY_BODY_STATUS: Record<BodyKind, BodyLoadStatus> = {
  request: "idle",
  response: "idle",
};
const EMPTY_DECODED_BODIES: Record<BodyKind, DecodedBodyState> = {
  request: EMPTY_DECODED_BODY,
  response: EMPTY_DECODED_BODY,
};

export function useRecordInspection({
  api,
  records,
  paused,
  onFailure,
  onRecovery,
}: UseRecordInspectionOptions) {
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
  const [failure, setFailure] = useState<InspectionFailure | null>(null);
  const failureRef = useRef<InspectionFailure | null>(null);
  const [bodyRetry, setBodyRetry] = useState(0);
  const detailController = useRef<AbortController | null>(null);
  const bodyController = useRef<AbortController | null>(null);
  const downloadController = useRef<AbortController | null>(null);
  const resumeDetailAfterPause = useRef(false);
  const refreshActiveAfterPause = useRef(false);
  const reportFailure = useCallback(
    (next: InspectionFailure) => {
      failureRef.current = next;
      setFailure(next);
      onFailure(next);
    },
    [onFailure],
  );
  const clearFailure = useCallback(
    (kind?: InspectionFailure["kind"]) => {
      if (kind && failureRef.current?.kind !== kind) return;
      if (failureRef.current === null) return;
      failureRef.current = null;
      setFailure(null);
      onRecovery();
    },
    [onRecovery],
  );
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
    clearFailure();
  }, [clearFailure]);
  const clearCurrentRecord = useCallback(() => {
    detailController.current?.abort();
    bodyController.current?.abort();
    detailController.current = null;
    bodyController.current = null;
    currentIdRef.current = null;
    resumeDetailAfterPause.current = false;
    refreshActiveAfterPause.current = false;
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
        clearFailure("detail");
      } catch (cause) {
        if (
          detailController.current === controller &&
          !requestWasCancelled(cause, controller.signal)
        ) {
          const notFound = isNotFound(cause);
          if (notFound) clearCurrentRecord();
          reportFailure({
            kind: "detail",
            message: requestErrorMessage(cause),
            retryable: !notFound,
          });
        }
      } finally {
        if (detailController.current === controller) setLoadingDetail(false);
      }
    },
    [api, clearCurrentRecord, clearFailure, records, reportFailure, resetRecordData],
  );

  useEffect(() => {
    if (!paused) return;
    resumeDetailAfterPause.current =
      resumeDetailAfterPause.current || (currentIdRef.current !== null && detail === null);
    refreshActiveAfterPause.current = refreshActiveAfterPause.current || detail?.state === "active";
    detailController.current?.abort();
    bodyController.current?.abort();
    downloadController.current?.abort();
    detailController.current = null;
    bodyController.current = null;
    downloadController.current = null;
    setLoadingDetail(false);
    setLoadingBody(false);
  }, [detail, paused]);

  useEffect(() => {
    if (paused || !resumeDetailAfterPause.current) return;
    const id = currentIdRef.current;
    resumeDetailAfterPause.current = false;
    if (id) void selectRecord(id);
  }, [paused, selectRecord]);

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
    if (paused || loadingDetail || !activeId) return;
    const id = activeId;
    const controller = detailController.current?.signal.aborted
      ? new AbortController()
      : (detailController.current ?? new AbortController());
    detailController.current = controller;
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
        clearFailure("detail");
        shouldContinue = fresh.state === "active";
      } catch (cause) {
        if (ownsRequest() && !requestWasCancelled(cause, controller.signal)) {
          if (isNotFound(cause)) {
            shouldContinue = false;
            clearCurrentRecord();
          }
          reportFailure({
            kind: "detail",
            message: requestErrorMessage(cause),
            retryable: !isNotFound(cause),
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
  }, [activeId, api, clearCurrentRecord, clearFailure, loadingDetail, paused, reportFailure]);

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
    if (paused || loadingDetail || detailState === null || !currentId || tab === "summary") return;
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
                error: `Decoded Source unavailable: ${requestErrorMessage(cause)}`,
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
              warning: `SSE Event timing unavailable: ${requestErrorMessage(cause)}`,
            }));
          }
        }
        clearFailure("body");
      } catch (cause) {
        if (ownsRequest() && !requestWasCancelled(cause, controller.signal)) {
          if (isNotFound(cause)) shouldContinue = false;
          setBodyStatus((current) => ({
            ...current,
            [kind]: current[kind] === "loaded" ? "loaded" : "error",
          }));
          reportFailure({ kind: "body", message: requestErrorMessage(cause), bodyKind: kind });
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
    bodyRetry,
    clearFailure,
    currentId,
    detailState,
    loadingDetail,
    paused,
    reportFailure,
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
      downloadController.current?.abort();
    },
    [],
  );

  const download = useCallback(
    async (kind: BodyKind) => {
      const id = currentIdRef.current;
      if (!id || paused) return;
      downloadController.current?.abort();
      const controller = new AbortController();
      downloadController.current = controller;
      clearFailure("download");
      try {
        const { bytes: data } = await api.loadBody(id, kind, 0, controller.signal);
        if (controller.signal.aborted || downloadController.current !== controller) return;
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
          downloadController.current === controller &&
          !requestWasCancelled(cause, controller.signal)
        ) {
          reportFailure({
            kind: "download",
            message: requestErrorMessage(cause),
            bodyKind: kind,
          });
        }
      } finally {
        if (downloadController.current === controller) downloadController.current = null;
      }
    },
    [api, clearFailure, paused, reportFailure],
  );

  const retryFailure = useCallback(() => {
    if (!failure) return;
    const current = failure;
    clearFailure();
    if (current.kind === "detail") {
      const id = currentIdRef.current;
      if (id) void selectRecord(id);
    } else if (current.kind === "body") {
      setBodyRetry((value) => value + 1);
    } else if (current.bodyKind) {
      void download(current.bodyKind);
    }
  }, [clearFailure, download, failure, selectRecord]);

  const selectTab = useCallback(
    (value: DetailTab) => {
      clearFailure();
      setTab(value);
    },
    [clearFailure],
  );

  return {
    bodies,
    bodyStatus,
    clearCurrentRecord,
    clearRecordIfCurrent,
    currentId,
    currentState,
    decodedBodies,
    detail,
    download,
    failure,
    eventTimings,
    loadingBody,
    loadingDetail,
    retryFailure,
    selectRecord,
    setTab: selectTab,
    syncCurrentState,
    tab,
  };
}

function isNotFound(cause: unknown): cause is ApiError {
  return cause instanceof ApiError && cause.status === 404;
}
