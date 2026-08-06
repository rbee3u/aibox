import { BookOpen, Box, GitFork, LoaderCircle, Radio } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createTrafficApi } from "./api";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { RecordDetail } from "./components/RecordDetail";
import { RecordList } from "./components/RecordList";
import { StatusBanner } from "./components/StatusBanner";
import type {
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
type BodyKind = "request" | "response";
type Dialog = { kind: "selected"; ids: string[] } | { kind: "all"; count: number } | null;
type ErrorSource = "list" | "detail" | "action";
type AppError = { source: ErrorSource; message: string };

export function App({ api: providedApi }: AppProps) {
  const api = useMemo(() => providedApi ?? createTrafficApi(), [providedApi]);
  const [list, setList] = useState<RecordListData>({
    records: [],
    total: 0,
    deletable_count: 0,
    next_cursor: null,
  });
  const [page, setPage] = useState(0);
  const pageRef = useRef(0);
  const cursors = useRef<Array<string | null>>([null]);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [currentId, setCurrentId] = useState<string | null>(null);
  const currentIdRef = useRef<string | null>(null);
  const [currentState, setCurrentState] = useState<RecordState | null>(null);
  const [detail, setDetail] = useState<RecordDetailData | null>(null);
  const [bodies, setBodies] = useState<{ request: Uint8Array[]; response: Uint8Array[] }>({
    request: [],
    response: [],
  });
  const offsets = useRef({ request: 0, response: 0 });
  const [tab, setTab] = useState<BodyKind>("request");
  const [loadingList, setLoadingList] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [loadingBody, setLoadingBody] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [deleting, setDeleting] = useState(false);
  const [deletingRecordId, setDeletingRecordId] = useState<string | null>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const listController = useRef<AbortController | null>(null);
  const pageNavigation = useRef(false);
  const detailController = useRef<AbortController | null>(null);
  const activeId = detail?.state === "active" ? detail.request.id : null;
  const reportError = useCallback((source: ErrorSource, cause: unknown) => {
    setError({
      source,
      message: typeof cause === "string" ? cause : errorMessage(cause),
    });
  }, []);
  const clearError = useCallback((source?: ErrorSource) => {
    setError((current) => (source === undefined || current?.source === source ? null : current));
  }, []);
  const exitSelectionMode = useCallback(() => {
    setSelectionMode(false);
    setSelected(new Set());
  }, []);
  const clearCurrentRecord = useCallback(() => {
    detailController.current?.abort();
    currentIdRef.current = null;
    setCurrentId(null);
    setCurrentState(null);
    setDetail(null);
  }, []);

  const loadPage = useCallback(
    async (pageToLoad: number, background = false): Promise<RecordListData | null> => {
      if (background && pageNavigation.current) return null;
      listController.current?.abort();
      const controller = new AbortController();
      listController.current = controller;
      if (!background) {
        pageNavigation.current = true;
        setLoadingList(true);
      }
      try {
        const payload = await api.listRecords(
          cursors.current[pageToLoad] ?? undefined,
          controller.signal,
        );
        if (listController.current !== controller || controller.signal.aborted) return null;
        setList(payload);
        setPage(pageToLoad);
        pageRef.current = pageToLoad;
        const currentSummary = currentIdRef.current
          ? payload.records.find((record) => record.id === currentIdRef.current)
          : undefined;
        if (currentSummary) {
          setCurrentState((current) =>
            current && current !== "active" ? current : currentSummary.state,
          );
        }
        if (payload.next_cursor) cursors.current[pageToLoad + 1] = payload.next_cursor;
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

  const refreshPage = useCallback(async () => {
    setRefreshing(true);
    try {
      await loadPage(page);
    } finally {
      setRefreshing(false);
    }
  }, [loadPage, page]);

  const refreshAfterDelete = useCallback(async () => {
    const pageToRefresh = pageRef.current;
    let payload = await loadPage(pageToRefresh);
    if (!payload) return null;
    if (pageToRefresh > 0 && payload.records.length === 0 && pageRef.current === pageToRefresh) {
      payload = await loadPage(pageToRefresh - 1);
      if (!payload) return null;
      return { page: pageToRefresh - 1, payload };
    }
    return { page: pageToRefresh, payload };
  }, [loadPage]);

  const loadBody = useCallback(
    async (id: string, kind: BodyKind, offset: number, controller: AbortController) => {
      const chunk = await api.loadBody(id, kind, offset, controller.signal);
      if (detailController.current !== controller || controller.signal.aborted) return;
      if (chunk.bytes.length > 0)
        setBodies((current) => ({ ...current, [kind]: [...current[kind], chunk.bytes] }));
      offsets.current[kind] = chunk.nextOffset;
    },
    [api],
  );

  const selectRecord = useCallback(
    async (id: string) => {
      detailController.current?.abort();
      const controller = new AbortController();
      detailController.current = controller;
      currentIdRef.current = id;
      setCurrentId(id);
      setCurrentState(list.records.find((record) => record.id === id)?.state ?? null);
      setDetail(null);
      setBodies({ request: [], response: [] });
      offsets.current = { request: 0, response: 0 };
      setTab("request");
      setLoadingDetail(true);
      clearError();
      try {
        const record = await api.getRecord(id, controller.signal);
        if (detailController.current !== controller || controller.signal.aborted) return;
        setDetail(record);
        setCurrentState(record.state);
        setLoadingBody(true);
        await Promise.all([
          loadBody(id, "request", 0, controller),
          loadBody(id, "response", 0, controller),
        ]);
      } catch (cause) {
        if (
          detailController.current === controller &&
          !(cause instanceof DOMException && cause.name === "AbortError")
        )
          reportError("detail", cause);
      } finally {
        if (detailController.current === controller) {
          setLoadingDetail(false);
          setLoadingBody(false);
        }
      }
    },
    [api, clearError, list.records, loadBody, reportError],
  );

  useEffect(() => {
    if (selectionMode) {
      listController.current?.abort();
      return;
    }
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      await loadPage(pageRef.current, true);
      if (!disposed) timer = window.setTimeout(() => void poll(), 2500);
    };
    void poll();
    return () => {
      disposed = true;
      listController.current?.abort();
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [loadPage, selectionMode]);

  useEffect(() => {
    if (!selectionMode) return;
    const exitOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || dialog !== null) return;
      event.preventDefault();
      exitSelectionMode();
    };
    window.addEventListener("keydown", exitOnEscape);
    return () => window.removeEventListener("keydown", exitOnEscape);
  }, [dialog, exitSelectionMode, selectionMode]);

  useEffect(
    () => () => {
      listController.current?.abort();
      detailController.current?.abort();
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
      setLoadingBody(true);
      try {
        await Promise.all([
          loadBody(currentId, "request", offsets.current.request, controller),
          loadBody(currentId, "response", offsets.current.response, controller),
        ]);
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
        if (detailController.current === controller) setLoadingBody(false);
        if (
          !disposed &&
          shouldContinue &&
          detailController.current === controller &&
          !controller.signal.aborted
        ) {
          timer = window.setTimeout(() => void poll(), 1000);
        }
      }
    };
    timer = window.setTimeout(() => void poll(), 1000);
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [activeId, api, clearError, currentId, loadBody, loadingDetail, reportError]);

  const selectedIds = useMemo(() => [...selected], [selected]);
  const togglePageSelection = () => {
    setSelected((current) => {
      const next = new Set(current);
      const deletableIds = list.records
        .filter((record) => record.state !== "active")
        .map((record) => record.id);
      const pageSelected = deletableIds.length > 0 && deletableIds.every((id) => current.has(id));
      deletableIds.forEach((id) => (pageSelected ? next.delete(id) : next.add(id)));
      return next;
    });
  };

  async function confirmDelete() {
    if (!dialog) return;
    setDeleting(true);
    try {
      if (dialog.kind === "selected") await api.deleteRecords(dialog.ids);
      else await api.deleteAll(dialog.count);
      const deletedIds = dialog.kind === "selected" ? dialog.ids : [];
      setSelected(new Set());
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
      await refreshAfterDelete();
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
      const stayedOnOriginPage = pageRef.current === originPage;
      const refreshed = await refreshAfterDelete();
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
        reportError("detail", cause);
    }
  }

  return (
    <div className={styles.app}>
      <header className={styles.topbar}>
        <div className={styles.brand}>
          <span className={styles.mark}>
            <Box size={23} strokeWidth={2.2} aria-hidden="true" />
          </span>
          <div className={styles.brandText} title="aibox traffic · Inspect LLM API activity">
            <strong>aibox traffic</strong>
            <span className={styles.separator}>·</span>
            <span className={styles.tagline}>Inspect LLM API activity</span>
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
        </nav>
      </header>
      {error && (
        <div className={styles.bannerWrap}>
          <StatusBanner
            kind="error"
            message={error.message}
            action={
              error.source === "list"
                ? selectionMode
                  ? undefined
                  : { label: "Retry", onClick: () => void refreshPage() }
                : error.source === "detail" && currentId
                  ? { label: "Retry", onClick: () => void selectRecord(currentId) }
                  : undefined
            }
            onDismiss={() => clearError()}
          />
        </div>
      )}
      <main className={styles.main}>
        <RecordList
          records={list.records}
          total={list.total}
          page={page}
          hasPrevious={page > 0}
          hasNext={Boolean(list.next_cursor)}
          selectionMode={selectionMode}
          selected={selected}
          currentId={currentId}
          onEnterSelection={() => setSelectionMode(true)}
          onExitSelection={exitSelectionMode}
          onTogglePage={togglePageSelection}
          onToggle={(id) =>
            setSelected((current) => {
              const next = new Set(current);
              if (next.has(id)) next.delete(id);
              else next.add(id);
              return next;
            })
          }
          onSelect={(id) => void selectRecord(id)}
          onPrevious={() => {
            cursors.current[page] = cursors.current[page] ?? null;
            void loadPage(page - 1);
          }}
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
        {loadingDetail ? (
          <section className={styles.emptyDetail}>
            <LoaderIcon />
            <p>Loading record…</p>
          </section>
        ) : detail ? (
          <RecordDetail
            detail={detail}
            bodies={bodies}
            tab={tab}
            onTabChange={setTab}
            onDownload={(kind) => void download(kind)}
            loadingBody={loadingBody}
          />
        ) : (
          <section className={styles.emptyDetail}>
            <Radio size={26} aria-hidden="true" />
            <h1>Select a request</h1>
            <p>Requests appear newest first. Bodies are loaded only when selected.</p>
          </section>
        )}
      </main>
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
function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : "Traffic management request failed";
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

export default App;
