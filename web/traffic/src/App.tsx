import { BookOpen, Box, GitFork, LoaderCircle, Radio, SunMoon } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, PointerEvent } from "react";
import { createTrafficApi } from "./api";
import { bodyComplete, contentCoding, isSseResponse } from "./bodyPresentation";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { RecordDetail } from "./components/RecordDetail";
import { RecordList } from "./components/RecordList";
import { StatusBanner } from "./components/StatusBanner";
import type {
  BodyKind,
  BodyLoadStatus,
  DecodedBodyState,
  DetailTab,
  EventTimingIndex,
  RecordDetail as RecordDetailData,
  RecordList as RecordListData,
  RecordSummary,
  RecordState,
  TrafficApi,
} from "./types";
import styles from "./App.module.css";

interface AppProps {
  api?: TrafficApi;
}
type Dialog = { kind: "selected"; ids: string[] } | { kind: "all"; count: number } | null;
type ErrorSource = "list" | "detail" | "body" | "action";
type AppErrors = Record<ErrorSource, string | null>;

const LIST_POLL_INTERVAL_MS = 5000;
const ACTIVE_DETAIL_POLL_INTERVAL_MS = 3000;
const RECORDS_PER_PAGE = 50;
const EMPTY_DECODED_BODY: DecodedBodyState = { bytes: null, status: "idle", message: null };
const THEME_STORAGE_KEY = "aibox-traffic-theme";
const LIST_WIDTH_STORAGE_KEY = "aibox-traffic-list-width";
const DEFAULT_LIST_WIDTH = 480;
const MIN_LIST_WIDTH = 360;
const MAX_LIST_WIDTH = 640;
const LIST_WIDTH_STEP = 16;
const EMPTY_ERRORS: AppErrors = { list: null, detail: null, body: null, action: null };

export type ThemePreference = "system" | "light" | "dark";

interface SplitDrag {
  pointerId: number;
  startX: number;
  startWidth: number;
  currentWidth: number;
}

export function App({ api: providedApi }: AppProps) {
  const api = useMemo(() => providedApi ?? createTrafficApi(), [providedApi]);
  const [theme, setTheme] = useState<ThemePreference>(readThemePreference);
  const [listWidth, setListWidth] = useState(readListWidth);
  const [resizing, setResizing] = useState(false);
  const splitDrag = useRef<SplitDrag | null>(null);
  const [list, setList] = useState<RecordListData>({
    records: [],
    total: 0,
    deletable_count: 0,
    has_next: false,
  });
  const [page, setPage] = useState(1);
  const pageRef = useRef(1);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const selectionPages = useRef<Map<string, number>>(new Map());
  const [currentId, setCurrentId] = useState<string | null>(null);
  const currentIdRef = useRef<string | null>(null);
  const [currentState, setCurrentState] = useState<RecordState | null>(null);
  const [detail, setDetail] = useState<RecordDetailData | null>(null);
  const [bodies, setBodies] = useState<{ request: Uint8Array[]; response: Uint8Array[] }>({
    request: [],
    response: [],
  });
  const [bodyStatus, setBodyStatus] = useState<Record<BodyKind, BodyLoadStatus>>({
    request: "idle",
    response: "idle",
  });
  const [decodedBodies, setDecodedBodies] = useState<Record<BodyKind, DecodedBodyState>>({
    request: EMPTY_DECODED_BODY,
    response: EMPTY_DECODED_BODY,
  });
  const [eventTimings, setEventTimings] = useState<EventTimingIndex | null>(null);
  const timingNextSequence = useRef(0);
  const offsets = useRef({ request: 0, response: 0 });
  const decodedLoaded = useRef({ request: false, response: false });
  const [tab, setTab] = useState<DetailTab>("summary");
  const [loadingList, setLoadingList] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [loadingBody, setLoadingBody] = useState(false);
  const [errors, setErrors] = useState<AppErrors>(EMPTY_ERRORS);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [deleting, setDeleting] = useState(false);
  const [deletingRecordId, setDeletingRecordId] = useState<string | null>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const listController = useRef<AbortController | null>(null);
  const pageNavigation = useRef(false);
  const detailController = useRef<AbortController | null>(null);
  const bodyController = useRef<AbortController | null>(null);
  const activeId = detail?.state === "active" ? detail.request.id : null;
  const detailState = detail?.state ?? null;
  const responseAvailable = detail?.response !== null && detail?.response !== undefined;
  const visibleBodyKind = tab === "summary" ? null : tab;
  const visibleBodyHeaders =
    visibleBodyKind === "request"
      ? detail?.request.headers
      : visibleBodyKind === "response"
        ? detail?.response?.headers
        : undefined;
  const visibleBodyCoding = contentCoding(visibleBodyHeaders ?? []);
  const visibleBodyCodingKind = visibleBodyCoding.kind;
  const visibleBodyCodingMessage =
    visibleBodyCoding.kind === "unsupported" ? visibleBodyCoding.message : null;
  const visibleBodyComplete =
    detail !== null && visibleBodyKind !== null ? bodyComplete(detail, visibleBodyKind) : false;
  const shouldLoadVisibleTimings =
    visibleBodyKind === "response" && detail !== null && isSseResponse(detail);
  const visibleDetailError = errors.detail ?? errors.body;
  const visibleDetailErrorSource: ErrorSource = errors.detail ? "detail" : "body";

  useLayoutEffect(() => {
    const root = document.documentElement;
    if (theme === "system") root.removeAttribute("data-theme");
    else root.dataset.theme = theme;
    storePreference(THEME_STORAGE_KEY, theme);
    return () => root.removeAttribute("data-theme");
  }, [theme]);

  const updateListWidth = useCallback((value: number, persist = false) => {
    const next = clampListWidth(value);
    setListWidth(next);
    if (persist) storePreference(LIST_WIDTH_STORAGE_KEY, String(next));
  }, []);

  function startResize(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    splitDrag.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: listWidth,
      currentWidth: listWidth,
    };
    setResizing(true);
    if (typeof event.currentTarget.setPointerCapture === "function") {
      event.currentTarget.setPointerCapture(event.pointerId);
    }
    event.preventDefault();
  }

  function resize(event: PointerEvent<HTMLDivElement>) {
    const drag = splitDrag.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    drag.currentWidth = clampListWidth(drag.startWidth + event.clientX - drag.startX);
    updateListWidth(drag.currentWidth);
  }

  function finishResize(event: PointerEvent<HTMLDivElement>) {
    const drag = splitDrag.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    updateListWidth(drag.currentWidth, true);
    splitDrag.current = null;
    setResizing(false);
    if (
      typeof event.currentTarget.hasPointerCapture === "function" &&
      event.currentTarget.hasPointerCapture(event.pointerId)
    ) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function resizeWithKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    let next: number | null = null;
    if (event.key === "ArrowLeft") next = listWidth - LIST_WIDTH_STEP;
    if (event.key === "ArrowRight") next = listWidth + LIST_WIDTH_STEP;
    if (event.key === "Home") next = MIN_LIST_WIDTH;
    if (event.key === "End") next = MAX_LIST_WIDTH;
    if (next === null) return;
    event.preventDefault();
    updateListWidth(next, true);
  }
  const reportError = useCallback((source: ErrorSource, cause: unknown) => {
    const message = typeof cause === "string" ? cause : errorMessage(cause);
    setErrors((current) => ({ ...current, [source]: message }));
  }, []);
  const clearError = useCallback((source: ErrorSource) => {
    setErrors((current) => (current[source] === null ? current : { ...current, [source]: null }));
  }, []);
  const exitSelectionMode = useCallback(() => {
    setSelectionMode(false);
    setSelected(new Set());
    selectionPages.current.clear();
  }, []);
  const clearCurrentRecord = useCallback(() => {
    detailController.current?.abort();
    bodyController.current?.abort();
    detailController.current = null;
    bodyController.current = null;
    currentIdRef.current = null;
    setCurrentId(null);
    setCurrentState(null);
    setDetail(null);
    setBodies({ request: [], response: [] });
    setBodyStatus({ request: "idle", response: "idle" });
    setDecodedBodies({ request: EMPTY_DECODED_BODY, response: EMPTY_DECODED_BODY });
    setEventTimings(null);
    timingNextSequence.current = 0;
    decodedLoaded.current = { request: false, response: false };
    offsets.current = { request: 0, response: 0 };
    setTab("summary");
    setLoadingDetail(false);
    setLoadingBody(false);
    clearError("detail");
    clearError("body");
  }, [clearError]);

  const loadPage = useCallback(
    async (pageToLoad: number, background = false): Promise<RecordListData | null> => {
      if (background && pageNavigation.current) return null;
      const targetPage = Math.max(1, pageToLoad);
      listController.current?.abort();
      const controller = new AbortController();
      listController.current = controller;
      if (!background) {
        pageNavigation.current = true;
        setLoadingList(true);
      }
      try {
        const payload = await api.listRecords(targetPage, controller.signal);
        if (listController.current !== controller || controller.signal.aborted) return null;
        setList(payload);
        setPage(targetPage);
        pageRef.current = targetPage;
        const currentSummary = currentIdRef.current
          ? payload.records.find((record) => record.id === currentIdRef.current)
          : undefined;
        if (currentSummary) {
          setCurrentState((current) =>
            current && current !== "active" ? current : currentSummary.state,
          );
        }
        clearError("list");
        return payload;
      } catch (cause) {
        if (
          listController.current === controller &&
          !(cause instanceof DOMException && cause.name === "AbortError")
        )
          reportError("list", cause);
        return null;
      } finally {
        if (listController.current === controller) {
          listController.current = null;
          if (!background) {
            pageNavigation.current = false;
            setLoadingList(false);
          }
        }
      }
    },
    [api, clearError, reportError],
  );

  const refreshWithFallback = useCallback(
    async (targetPage = pageRef.current, background = false) => {
      let candidate = Math.max(1, targetPage);
      while (true) {
        const payload = await loadPage(candidate, background);
        if (!payload) return null;
        if (payload.records.length > 0 || candidate === 1) return { page: candidate, payload };
        candidate -= 1;
      }
    },
    [loadPage],
  );

  const refreshPage = useCallback(async () => {
    setRefreshing(true);
    try {
      await refreshWithFallback(page);
    } finally {
      setRefreshing(false);
    }
  }, [page, refreshWithFallback]);

  const loadBody = useCallback(
    async (id: string, kind: BodyKind, offset: number, controller: AbortController) => {
      const chunk = await api.loadBody(id, kind, offset, controller.signal);
      if (
        bodyController.current !== controller ||
        currentIdRef.current !== id ||
        controller.signal.aborted
      )
        return;
      if (chunk.bytes.length > 0)
        setBodies((current) => ({ ...current, [kind]: [...current[kind], chunk.bytes] }));
      offsets.current[kind] = chunk.nextOffset;
    },
    [api],
  );

  const selectRecord = useCallback(
    async (id: string) => {
      detailController.current?.abort();
      bodyController.current?.abort();
      const controller = new AbortController();
      detailController.current = controller;
      currentIdRef.current = id;
      setCurrentId(id);
      setCurrentState(list.records.find((record) => record.id === id)?.state ?? null);
      setDetail(null);
      setBodies({ request: [], response: [] });
      setBodyStatus({ request: "idle", response: "idle" });
      setDecodedBodies({ request: EMPTY_DECODED_BODY, response: EMPTY_DECODED_BODY });
      setEventTimings(null);
      timingNextSequence.current = 0;
      decodedLoaded.current = { request: false, response: false };
      offsets.current = { request: 0, response: 0 };
      setTab("summary");
      setLoadingBody(false);
      setLoadingDetail(true);
      clearError("detail");
      clearError("body");
      try {
        const record = await api.getRecord(id, controller.signal);
        if (detailController.current !== controller || controller.signal.aborted) return;
        setDetail(record);
        setCurrentState(record.state);
      } catch (cause) {
        if (
          detailController.current === controller &&
          !(cause instanceof DOMException && cause.name === "AbortError")
        )
          reportError("detail", cause);
      } finally {
        if (detailController.current === controller) {
          setLoadingDetail(false);
        }
      }
    },
    [api, clearError, list.records, reportError],
  );

  useEffect(() => {
    if (selectionMode) {
      listController.current?.abort();
      return;
    }
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      await refreshWithFallback(pageRef.current, true);
      if (!disposed) timer = window.setTimeout(() => void poll(), LIST_POLL_INTERVAL_MS);
    };
    void poll();
    return () => {
      disposed = true;
      listController.current?.abort();
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [refreshWithFallback, selectionMode]);

  useEffect(
    () => () => {
      listController.current?.abort();
      detailController.current?.abort();
      bodyController.current?.abort();
    },
    [],
  );

  useEffect(() => {
    if (loadingDetail || !activeId || !currentId) return;
    const controller = detailController.current;
    if (!controller) return;
    let disposed = false;
    let timer: number | undefined;
    let shouldContinue = true;
    const poll = async () => {
      if (disposed || detailController.current !== controller || controller.signal.aborted) return;
      try {
        const fresh = await api.getRecord(currentId, controller.signal);
        if (disposed || detailController.current !== controller || controller.signal.aborted)
          return;
        setDetail(fresh);
        setCurrentState(fresh.state);
        clearError("detail");
        shouldContinue = fresh.state === "active";
      } catch (cause) {
        if (
          detailController.current === controller &&
          !(cause instanceof DOMException && cause.name === "AbortError")
        )
          reportError("detail", cause);
      } finally {
        if (
          !disposed &&
          shouldContinue &&
          detailController.current === controller &&
          !controller.signal.aborted
        ) {
          timer = window.setTimeout(() => void poll(), ACTIVE_DETAIL_POLL_INTERVAL_MS);
        }
      }
    };
    timer = window.setTimeout(() => void poll(), ACTIVE_DETAIL_POLL_INTERVAL_MS);
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [activeId, api, clearError, currentId, loadingDetail, reportError]);

  useEffect(() => {
    bodyController.current?.abort();
    if (loadingDetail || detailState === null || !currentId || tab === "summary") return;
    if (tab === "response" && !responseAvailable) {
      return;
    }

    const id = currentId;
    const kind = tab;
    const active = detailState === "active";
    const controller = new AbortController();
    bodyController.current = controller;
    let disposed = false;
    let timer: number | undefined;

    const poll = async () => {
      if (disposed || bodyController.current !== controller || controller.signal.aborted) return;
      setLoadingBody(true);
      setBodyStatus((current) => ({
        ...current,
        [kind]: current[kind] === "loaded" ? "loaded" : "loading",
      }));
      try {
        await loadBody(id, kind, offsets.current[kind], controller);
        if (disposed || bodyController.current !== controller || controller.signal.aborted) return;
        setBodyStatus((current) => ({ ...current, [kind]: "loaded" }));
        if (visibleBodyCodingKind === "unsupported") {
          setDecodedBodies((current) => ({
            ...current,
            [kind]: {
              bytes: null,
              status: "unsupported",
              message: visibleBodyCodingMessage,
            },
          }));
        } else if (visibleBodyCodingKind === "zstd" && !visibleBodyComplete) {
          setDecodedBodies((current) => ({
            ...current,
            [kind]: {
              bytes: null,
              status: "waiting",
              message: "Waiting for the complete zstd body before decoding.",
            },
          }));
        } else if (visibleBodyCodingKind === "zstd" && !decodedLoaded.current[kind]) {
          setDecodedBodies((current) => ({
            ...current,
            [kind]: { bytes: null, status: "loading", message: null },
          }));
          try {
            const decoded = await api.loadDecodedBody(id, kind, controller.signal);
            if (
              disposed ||
              bodyController.current !== controller ||
              controller.signal.aborted ||
              currentIdRef.current !== id
            )
              return;
            decodedLoaded.current[kind] = true;
            setDecodedBodies((current) => ({
              ...current,
              [kind]: { bytes: decoded, status: "loaded", message: null },
            }));
          } catch (cause) {
            if (!(cause instanceof DOMException && cause.name === "AbortError")) {
              decodedLoaded.current[kind] = false;
              setDecodedBodies((current) => ({
                ...current,
                [kind]: {
                  bytes: null,
                  status: "error",
                  message: `Decoded Source unavailable: ${errorMessage(cause)}`,
                },
              }));
            }
          }
        } else if (visibleBodyCodingKind === "identity") {
          setDecodedBodies((current) => ({
            ...current,
            [kind]: { bytes: null, status: "loaded", message: null },
          }));
        }

        if (shouldLoadVisibleTimings) {
          try {
            const timing = await api.loadEventTimings(
              id,
              timingNextSequence.current,
              controller.signal,
            );
            if (disposed || bodyController.current !== controller || controller.signal.aborted)
              return;
            timingNextSequence.current = Math.max(timingNextSequence.current, timing.next_sequence);
            setEventTimings((current) => mergeEventTimings(current, timing));
          } catch (cause) {
            if (!(cause instanceof DOMException && cause.name === "AbortError")) {
              setEventTimings((current) => ({
                state: "unavailable",
                events: current?.events ?? [],
                next_sequence: timingNextSequence.current,
                warning: `SSE Event timing unavailable: ${errorMessage(cause)}`,
              }));
            }
          }
        }
        clearError("body");
      } catch (cause) {
        if (
          bodyController.current === controller &&
          !(cause instanceof DOMException && cause.name === "AbortError")
        ) {
          setBodyStatus((current) => ({
            ...current,
            [kind]: current[kind] === "loaded" ? "loaded" : "error",
          }));
          reportError("body", cause);
        }
      } finally {
        if (bodyController.current === controller) setLoadingBody(false);
        if (
          !disposed &&
          active &&
          !visibleBodyComplete &&
          bodyController.current === controller &&
          !controller.signal.aborted
        ) {
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
    clearError,
    currentId,
    detailState,
    api,
    loadBody,
    loadingDetail,
    reportError,
    responseAvailable,
    shouldLoadVisibleTimings,
    tab,
    visibleBodyCodingKind,
    visibleBodyCodingMessage,
    visibleBodyComplete,
  ]);

  const selectedIds = useMemo(() => [...selected], [selected]);
  const togglePageSelection = () => {
    setSelected((current) => {
      const next = new Set(current);
      const deletableIds = list.records
        .filter((record) => record.state !== "active")
        .map((record) => record.id);
      const pageSelected = deletableIds.length > 0 && deletableIds.every((id) => current.has(id));
      deletableIds.forEach((id) => {
        if (pageSelected) {
          next.delete(id);
          selectionPages.current.delete(id);
        } else {
          if (!next.has(id)) {
            next.add(id);
            selectionPages.current.set(id, pageRef.current);
          }
        }
      });
      return next;
    });
  };

  async function confirmDelete() {
    if (!dialog) return;
    const targetPage =
      dialog.kind === "selected"
        ? dialog.ids.reduce(
            (minimum, id) => Math.min(minimum, selectionPages.current.get(id) ?? pageRef.current),
            Number.MAX_SAFE_INTEGER,
          )
        : pageRef.current;
    setDeleting(true);
    try {
      if (dialog.kind === "selected") await api.deleteRecords(dialog.ids);
      else await api.deleteAll(dialog.count);
      const deletedIds = dialog.kind === "selected" ? dialog.ids : [];
      setList((current) =>
        removeDeletedFromList(
          current,
          dialog.kind === "all"
            ? current.records
                .filter((record) => record.state !== "active")
                .map((record) => record.id)
            : deletedIds,
          dialog.kind === "all" ? dialog.count : deletedIds.length,
          pageRef.current,
        ),
      );
      setSelected(new Set());
      selectionPages.current.clear();
      if (dialog.kind === "selected") setSelectionMode(false);
      if (
        currentId &&
        (deletedIds.includes(currentId) ||
          (dialog.kind === "all" && currentState !== null && currentState !== "active"))
      ) {
        clearCurrentRecord();
      }
      setDialog(null);
      clearError("action");
      await refreshWithFallback(targetPage);
      setFocusAfterDelete(null);
    } catch (cause) {
      reportError("action", cause);
    } finally {
      setDeleting(false);
    }
  }

  async function deleteRecord(id: string) {
    const originPage = pageRef.current;
    const originRecords = list.records;
    setDeletingRecordId(id);
    clearError("action");
    try {
      await api.deleteRecords([id]);
      if (currentIdRef.current === id) clearCurrentRecord();
      setList((current) => removeDeletedFromList(current, [id], 1, pageRef.current));
      const stayedOnOriginPage = pageRef.current === originPage;
      const refreshed = await refreshWithFallback(originPage);
      if (stayedOnOriginPage && refreshed) {
        setFocusAfterDelete(
          focusTargetAfterDelete(
            originRecords,
            id,
            refreshed.payload.records,
            refreshed.page !== originPage,
          ),
        );
      }
    } catch (cause) {
      reportError("action", cause);
    } finally {
      setDeletingRecordId(null);
    }
  }

  async function download(kind: BodyKind) {
    const id = currentId;
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
        !(cause instanceof DOMException && cause.name === "AbortError")
      )
        reportError("body", cause);
    }
  }

  return (
    <div className={styles.app}>
      <header className={styles.topbar}>
        <div className={styles.brand}>
          <span className={styles.mark}>
            <Box size={23} strokeWidth={2.2} aria-hidden="true" />
          </span>
          <div className={styles.brandText} title="AIBox Traffic · Inspect your LLM requests">
            <strong>AIBox Traffic</strong>
            <span className={styles.separator}>·</span>
            <span className={styles.tagline}>Inspect your LLM requests</span>
          </div>
        </div>
        <nav className={styles.resources} aria-label="Resources">
          <a
            href="https://developers.openai.com/codex/cli"
            target="_blank"
            rel="noopener noreferrer"
          >
            <BookOpen size={14} aria-hidden="true" /> Codex docs
          </a>
          <a
            href="https://code.claude.com/docs/en/overview"
            target="_blank"
            rel="noopener noreferrer"
          >
            <BookOpen size={14} aria-hidden="true" /> Claude docs
          </a>
          <a href="https://github.com/rbee3u/aibox" target="_blank" rel="noopener noreferrer">
            <GitFork size={14} aria-hidden="true" /> GitHub
          </a>
          <label className={styles.themeControl}>
            <SunMoon size={14} aria-hidden="true" />
            <span className="srOnly">Color theme</span>
            <select
              aria-label="Color theme"
              value={theme}
              onChange={(event) => setTheme(event.target.value as ThemePreference)}
            >
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>
        </nav>
      </header>
      <main
        className={`${styles.main} ${resizing ? styles.resizing : ""}`}
        style={{ "--list-width": `${listWidth}px` } as CSSProperties}
      >
        <div className={styles.listColumn}>
          {errors.list && (
            <div className={styles.scopedBanner}>
              <StatusBanner
                kind="error"
                message={errors.list}
                action={
                  selectionMode ? undefined : { label: "Retry", onClick: () => void refreshPage() }
                }
                onDismiss={() => clearError("list")}
              />
            </div>
          )}
          <RecordList
            records={list.records}
            total={list.total}
            page={page}
            hasPrevious={page > 1}
            hasNext={list.has_next}
            selectionMode={selectionMode}
            selected={selected}
            currentId={currentId}
            onEnterSelection={() => setSelectionMode(true)}
            onExitSelection={exitSelectionMode}
            onTogglePage={togglePageSelection}
            onToggle={(id) =>
              setSelected((current) => {
                const next = new Set(current);
                if (next.has(id)) {
                  next.delete(id);
                  selectionPages.current.delete(id);
                } else {
                  next.add(id);
                  selectionPages.current.set(id, pageRef.current);
                }
                return next;
              })
            }
            onSelect={(id) => void selectRecord(id)}
            onPrevious={() => void loadPage(page - 1)}
            onNext={() => void loadPage(page + 1)}
            loading={loadingList}
            refreshing={refreshing}
            deletableCount={list.deletable_count}
            onRefresh={() => void refreshPage()}
            onDeleteSelected={() => setDialog({ kind: "selected", ids: selectedIds })}
            onDeleteAll={() => setDialog({ kind: "all", count: list.deletable_count })}
            onDeleteRecord={(id) => void deleteRecord(id)}
            deletingRecordId={deletingRecordId}
            deletionBusy={deleting || deletingRecordId !== null}
            focusAfterDelete={focusAfterDelete}
            onFocusAfterDelete={() => setFocusAfterDelete(undefined)}
          />
        </div>
        <div
          className={styles.splitter}
          role="separator"
          aria-label="Resize Traffic records panel"
          aria-orientation="vertical"
          aria-valuemin={MIN_LIST_WIDTH}
          aria-valuemax={MAX_LIST_WIDTH}
          aria-valuenow={listWidth}
          tabIndex={0}
          onPointerDown={startResize}
          onPointerMove={resize}
          onPointerUp={finishResize}
          onPointerCancel={finishResize}
          onDoubleClick={() => updateListWidth(DEFAULT_LIST_WIDTH, true)}
          onKeyDown={resizeWithKeyboard}
        >
          <span aria-hidden="true" />
        </div>
        <div className={styles.detailColumn}>
          {visibleDetailError && (
            <div className={styles.scopedBanner}>
              <StatusBanner
                kind="error"
                message={visibleDetailError}
                action={
                  currentId
                    ? { label: "Retry", onClick: () => void selectRecord(currentId) }
                    : undefined
                }
                onDismiss={() => clearError(visibleDetailErrorSource)}
              />
            </div>
          )}
          {loadingDetail ? (
            <section className={styles.emptyDetail}>
              <LoaderIcon />
              <p>Loading record…</p>
            </section>
          ) : detail ? (
            <RecordDetail
              key={detail.request.id}
              detail={detail}
              bodies={bodies}
              bodyStatus={bodyStatus}
              decodedBodies={decodedBodies}
              eventTimings={eventTimings}
              tab={tab}
              onTabChange={setTab}
              onDownload={(kind) => void download(kind)}
              loadingBody={loadingBody}
            />
          ) : (
            <section className={styles.emptyDetail}>
              <Radio size={26} aria-hidden="true" />
              <h1>Select a request</h1>
              <p>Choose a Traffic Record to inspect its summary and raw data.</p>
            </section>
          )}
        </div>
      </main>
      {errors.action && (
        <div className={styles.actionNotice}>
          <StatusBanner
            kind="error"
            message={errors.action}
            onDismiss={() => clearError("action")}
          />
        </div>
      )}
      {dialog && (
        <ConfirmDialog
          title={
            dialog.kind === "all"
              ? "Delete all records?"
              : `Delete ${dialog.ids.length} selected record${dialog.ids.length === 1 ? "" : "s"}?`
          }
          message={
            dialog.kind === "all"
              ? `This permanently deletes ${dialog.count} non-active record${dialog.count === 1 ? "" : "s"}, including raw request and response data.`
              : "This permanently deletes the selected raw request and response data."
          }
          confirmLabel="Delete permanently"
          onConfirm={() => void confirmDelete()}
          onCancel={() => !deleting && setDialog(null)}
          busy={deleting}
        />
      )}
    </div>
  );
}

function LoaderIcon() {
  return <LoaderCircle className={styles.loader} size={28} aria-label="Loading" />;
}

function readThemePreference(): ThemePreference {
  const value = readPreference(THEME_STORAGE_KEY);
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}

function readListWidth(): number {
  const stored = readPreference(LIST_WIDTH_STORAGE_KEY);
  if (stored === null) return DEFAULT_LIST_WIDTH;
  const value = Number(stored);
  return Number.isFinite(value) ? clampListWidth(value) : DEFAULT_LIST_WIDTH;
}

function clampListWidth(value: number): number {
  return Math.min(MAX_LIST_WIDTH, Math.max(MIN_LIST_WIDTH, Math.round(value)));
}

function readPreference(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storePreference(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Appearance preferences are optional; the viewer remains fully usable without storage.
  }
}
function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : "Traffic management request failed";
}

function mergeEventTimings(
  current: EventTimingIndex | null,
  incoming: EventTimingIndex,
): EventTimingIndex {
  const bySequence = new Map(
    (current?.events ?? []).map((event) => [event.sequence, event] as const),
  );
  for (const event of incoming.events) bySequence.set(event.sequence, event);
  return {
    state:
      current?.state === "partial" || incoming.state === "partial" ? "partial" : incoming.state,
    events: [...bySequence.values()].sort((left, right) => left.sequence - right.sequence),
    next_sequence: Math.max(current?.next_sequence ?? 0, incoming.next_sequence),
    warning: incoming.warning ?? current?.warning ?? null,
  };
}

function focusTargetAfterDelete(
  before: RecordSummary[],
  deletedId: string,
  after: RecordSummary[],
  movedToPreviousPage: boolean,
): string | null {
  const deletableAfter = after.filter((record) => record.state !== "active");
  if (deletableAfter.length === 0) return null;
  if (movedToPreviousPage) return deletableAfter.at(-1)?.id ?? null;

  const deletedIndex = before.findIndex((record) => record.id === deletedId);
  if (deletedIndex >= 0) {
    const adjacentIds = [
      ...before.slice(deletedIndex + 1),
      ...before.slice(0, deletedIndex).reverse(),
    ]
      .filter((record) => record.state !== "active")
      .map((record) => record.id);
    const remainingIds = new Set(deletableAfter.map((record) => record.id));
    const adjacentId = adjacentIds.find((id) => remainingIds.has(id));
    if (adjacentId) return adjacentId;
  }

  const start = Math.min(Math.max(deletedIndex, 0), after.length - 1);
  return (
    after.slice(start).find((record) => record.state !== "active")?.id ??
    after
      .slice(0, start)
      .reverse()
      .find((record) => record.state !== "active")?.id ??
    null
  );
}

function removeDeletedFromList(
  current: RecordListData,
  ids: readonly string[],
  deletedCount: number,
  currentPage: number,
): RecordListData {
  const deleted = new Set(ids);
  const total = Math.max(0, current.total - deletedCount);
  return {
    ...current,
    records: current.records.filter((record) => !deleted.has(record.id)),
    total,
    deletable_count: Math.max(0, current.deletable_count - deletedCount),
    has_next: currentPage * RECORDS_PER_PAGE < total,
  };
}

export default App;
