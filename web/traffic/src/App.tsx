import { BookOpen, Box, GitFork, LoaderCircle, Radio, SunMoon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { createTrafficApi } from "./api";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { RecordDetail } from "./components/RecordDetail";
import { RecordList } from "./components/RecordList";
import { StatusBanner } from "./components/StatusBanner";
import { useRecordInspection } from "./useRecordInspection";
import { MAX_LIST_WIDTH, MIN_LIST_WIDTH, useResizableListWidth } from "./useResizableListWidth";
import { usePersistentTheme, type ThemePreference } from "./usePersistentTheme";
import type { RecordList as RecordListData, RecordSummary, TrafficApi } from "./types";
import styles from "./App.module.css";

interface AppProps {
  api?: TrafficApi;
}
type Dialog = { kind: "selected"; ids: string[] } | { kind: "all"; count: number } | null;
type Deletion = { kind: "batch" } | { kind: "record"; id: string } | null;
type ErrorSource = "list" | "action";
type AppErrors = Record<ErrorSource, string | null>;

const LIST_POLL_INTERVAL_MS = 5000;
const RECORDS_PER_PAGE = 50;
const EMPTY_ERRORS: AppErrors = { list: null, action: null };

export function App({ api: providedApi }: AppProps) {
  const api = useMemo(() => providedApi ?? createTrafficApi(), [providedApi]);
  const [theme, setTheme] = usePersistentTheme();
  const {
    listWidth,
    resizing,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onKeyDown,
    reset: resetListWidth,
  } = useResizableListWidth();
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
  const [loadingList, setLoadingList] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [errors, setErrors] = useState<AppErrors>(EMPTY_ERRORS);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [deletion, setDeletion] = useState<Deletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const listController = useRef<AbortController | null>(null);
  const deletionInProgress = useRef(false);
  const pageNavigation = useRef(false);
  const deleting = deletion?.kind === "batch";
  const deletingRecordId = deletion?.kind === "record" ? deletion.id : null;
  const deletionBusy = deletion !== null;

  const reportError = useCallback((source: ErrorSource, cause: unknown) => {
    const message = typeof cause === "string" ? cause : errorMessage(cause);
    setErrors((current) => ({ ...current, [source]: message }));
  }, []);
  const clearError = useCallback((source: ErrorSource) => {
    setErrors((current) => (current[source] === null ? current : { ...current, [source]: null }));
  }, []);
  const inspection = useRecordInspection({ api, records: list.records });
  const {
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
    eventTimings,
    error: inspectionError,
    loadingBody,
    loadingDetail,
    selectRecord,
    setTab,
    syncCurrentState,
    tab,
  } = inspection;
  const exitSelectionMode = useCallback(() => {
    setSelectionMode(false);
    setSelected(new Set());
    selectionPages.current.clear();
  }, []);
  function beginDeletion(next: Exclude<Deletion, null>): boolean {
    if (deletionInProgress.current) return false;
    deletionInProgress.current = true;
    listController.current?.abort();
    setDeletion(next);
    return true;
  }

  function finishDeletion() {
    deletionInProgress.current = false;
    setDeletion(null);
  }

  const loadPage = useCallback(
    async (pageToLoad: number, background = false): Promise<RecordListData | null> => {
      if (background && (pageNavigation.current || deletionInProgress.current)) return null;
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
        syncCurrentState(payload.records);
        clearError("list");
        return payload;
      } catch (cause) {
        if (listController.current === controller && !requestWasCancelled(cause, controller.signal))
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
    [api, clearError, reportError, syncCurrentState],
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

  const selectedIds = [...selected];

  function toggleRecordSelection(id: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.delete(id)) {
        selectionPages.current.delete(id);
      } else {
        next.add(id);
        selectionPages.current.set(id, pageRef.current);
      }
      return next;
    });
  }

  function togglePageSelection() {
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
        } else if (!next.has(id)) {
          next.add(id);
          selectionPages.current.set(id, pageRef.current);
        }
      });
      return next;
    });
  }

  async function confirmDelete() {
    if (!dialog || !beginDeletion({ kind: "batch" })) return;
    const targetPage =
      dialog.kind === "selected"
        ? dialog.ids.reduce(
            (minimum, id) => Math.min(minimum, selectionPages.current.get(id) ?? pageRef.current),
            Number.MAX_SAFE_INTEGER,
          )
        : pageRef.current;
    try {
      const deletedCount =
        dialog.kind === "selected"
          ? await api.deleteRecords(dialog.ids)
          : await api.deleteAll(dialog.count);
      const deletedIds = dialog.kind === "selected" ? dialog.ids : [];
      setList((current) =>
        removeDeletedFromList(
          current,
          dialog.kind === "all"
            ? current.records
                .filter((record) => record.state !== "active")
                .map((record) => record.id)
            : deletedIds,
          deletedCount,
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
      finishDeletion();
    }
  }

  async function deleteRecord(id: string) {
    if (!beginDeletion({ kind: "record", id })) return;
    const originPage = pageRef.current;
    const originRecords = list.records;
    clearError("action");
    try {
      await api.deleteRecords([id]);
      clearRecordIfCurrent(id);
      setList((current) => removeDeletedFromList(current, [id], 1, pageRef.current));
      setFocusAfterDelete(
        focusTargetAfterDelete(
          originRecords,
          id,
          originRecords.filter((record) => record.id !== id),
          false,
        ),
      );
      const refreshed = await refreshWithFallback(originPage);
      if (refreshed) {
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
      finishDeletion();
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
            onToggle={toggleRecordSelection}
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
            deletionBusy={deletionBusy}
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
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          onDoubleClick={resetListWidth}
          onKeyDown={onKeyDown}
        >
          <span aria-hidden="true" />
        </div>
        <div className={styles.detailColumn}>
          {inspectionError && (
            <div className={styles.scopedBanner}>
              <StatusBanner
                message={inspectionError}
                action={
                  currentId
                    ? { label: "Retry", onClick: () => void selectRecord(currentId) }
                    : undefined
                }
                onDismiss={clearVisibleError}
              />
            </div>
          )}
          {loadingDetail ? (
            <section className={styles.emptyDetail}>
              <LoaderCircle className={styles.loader} size={28} aria-label="Loading" />
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
          <StatusBanner message={errors.action} onDismiss={() => clearError("action")} />
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

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : "Traffic management request failed";
}

function requestWasCancelled(cause: unknown, signal: AbortSignal): boolean {
  return (
    signal.aborted ||
    (typeof cause === "object" && cause !== null && "name" in cause && cause.name === "AbortError")
  );
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
