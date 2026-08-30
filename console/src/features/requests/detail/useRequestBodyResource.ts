import { useCallback, useEffect, useRef, useState } from "react";
import {
  ApiError,
  type BodyKind,
  type EventTimingIndex,
  type RequestDetail,
  type RequestsApi,
} from "@/api/requests";
import {
  bodyComplete,
  bodyHeaders,
  contentCoding,
  isSseResponse,
} from "@/features/requests/detail/bodyPresentation";
import type {
  ClearInspectionFailure,
  ReportInspectionFailure,
  RequestInspectionIdentity,
} from "@/features/requests/detail/inspectionTypes";
import { requestErrorMessage, requestWasCancelled } from "@/features/requests/requestErrors";
import { mergeEventTimings } from "@/features/requests/requestFormat";
import type { BodyLoadStatus, DecodedBodyState, DetailTab } from "@/features/requests/viewTypes";

const ACTIVE_BODY_POLL_INTERVAL_MS = 3000;
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

interface BodyResourceOptions {
  api: RequestsApi;
  clearFailure: ClearInspectionFailure;
  detail: RequestDetail | null;
  identity: RequestInspectionIdentity | null;
  loadingDetail: boolean;
  paused: boolean;
  reportFailure: ReportInspectionFailure;
  tab: DetailTab;
}

export function useRequestBodyResource({
  api,
  clearFailure,
  detail,
  identity,
  loadingDetail,
  paused,
  reportFailure,
  tab,
}: BodyResourceOptions) {
  const identityRef = useRef(identity);
  useEffect(() => {
    identityRef.current = identity;
  }, [identity]);
  const [bodies, setBodies] = useState(EMPTY_BODIES);
  const [bodyStatus, setBodyStatus] = useState(EMPTY_BODY_STATUS);
  const [decodedBodies, setDecodedBodies] = useState(EMPTY_DECODED_BODIES);
  const [eventTimings, setEventTimings] = useState<EventTimingIndex | null>(null);
  const [loadingBody, setLoadingBody] = useState(false);
  const [retryGeneration, setRetryGeneration] = useState(0);
  const timingNextSequence = useRef(0);
  const offsets = useRef({ request: 0, response: 0 });
  const decodedLoaded = useRef({ request: false, response: false });
  const controllerRef = useRef<AbortController | null>(null);

  const reset = useCallback(() => {
    controllerRef.current?.abort();
    controllerRef.current = null;
    setBodies(EMPTY_BODIES);
    setBodyStatus(EMPTY_BODY_STATUS);
    setDecodedBodies(EMPTY_DECODED_BODIES);
    setEventTimings(null);
    timingNextSequence.current = 0;
    decodedLoaded.current = { request: false, response: false };
    offsets.current = { request: 0, response: 0 };
    setLoadingBody(false);
  }, []);

  useEffect(() => {
    if (!paused) return;
    controllerRef.current?.abort();
    controllerRef.current = null;
    setLoadingBody(false);
  }, [paused]);

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
    controllerRef.current?.abort();
    if (paused || loadingDetail || detailState === null || !identity || tab === "summary") return;
    if (tab === "response" && !responseAvailable) return;

    const selected = identity;
    const kind = tab;
    const active = detailState === "active";
    const controller = new AbortController();
    controllerRef.current = controller;
    let disposed = false;
    let timer: number | undefined;
    let shouldContinue = active && !visibleBodyComplete;
    const ownsRequest = () =>
      !disposed &&
      controllerRef.current === controller &&
      identityRef.current?.generation === selected.generation &&
      !controller.signal.aborted;

    const poll = async () => {
      if (!ownsRequest()) return;
      setLoadingBody(true);
      try {
        const chunk = await api.loadBody(
          selected.id,
          kind,
          offsets.current[kind],
          controller.signal,
        );
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
            const decoded = await api.loadDecodedBody(selected.id, kind, controller.signal);
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
              selected.id,
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
        if (controllerRef.current === controller) setLoadingBody(false);
        if (shouldContinue && ownsRequest()) {
          timer = window.setTimeout(() => void poll(), ACTIVE_BODY_POLL_INTERVAL_MS);
        }
      }
    };

    void poll();
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
      controller.abort();
      if (controllerRef.current === controller) {
        controllerRef.current = null;
        setLoadingBody(false);
      }
    };
  }, [
    api,
    clearFailure,
    detailState,
    identity,
    loadingDetail,
    paused,
    reportFailure,
    responseAvailable,
    retryGeneration,
    shouldLoadVisibleTimings,
    tab,
    visibleBodyCodingKind,
    visibleBodyComplete,
  ]);

  useEffect(() => () => controllerRef.current?.abort(), []);

  return {
    bodies,
    bodyStatus,
    decodedBodies,
    eventTimings,
    loadingBody,
    reset,
    retry: () => setRetryGeneration((value) => value + 1),
  };
}

function isNotFound(cause: unknown): cause is ApiError {
  return cause instanceof ApiError && cause.status === 404;
}
