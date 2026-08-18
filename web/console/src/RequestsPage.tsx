import {
  ArrowLeftRight,
  Box,
  ChevronLeft,
  CircleAlert,
  GitFork,
  LoaderCircle,
  SunMoon,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRequestApi, requestWasCancelled } from "./api";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { NotificationCenter, type NotificationItemData } from "./components/NotificationCenter";
import { RecordDetail } from "./components/RecordDetail";
import { RecordList } from "./components/RecordList";
import { useFailureNotifications } from "./useFailureNotifications";
import { useRecordInspection, type InspectionFailure } from "./useRecordInspection";
import { usePersistentTheme, type ThemePreference } from "./usePersistentTheme";
import { AgentIcon } from "./icons";
import type { RecordList as RecordListData, RecordSummary, RequestApi } from "./types";
import styles from "./RequestsPage.module.css";

interface RequestsPageProps {
  api?: RequestApi;
  standalone?: boolean;
}
type Dialog = { ids: string[] } | null;
type Deletion = { kind: "batch" } | { kind: "record"; id: string } | null;

const LIST_POLL_INTERVAL_MS = 5000;
const RECORDS_PER_PAGE = 50;

function StandaloneHeader() {
  const [theme, setTheme] = usePersistentTheme();

  return (
    <header className={styles.topbar}>
      <div className={styles.brand}>
        <span className={styles.mark}>
          <Box size={23} strokeWidth={2.2} aria-hidden="true" />
        </span>
        <div className={styles.brandText} title="AIBox Requests · Inspect your LLM requests">
          <strong>AIBox Requests</strong>
          <span className={styles.separator}>·</span>
          <span className={styles.tagline}>Inspect your LLM requests</span>
        </div>
      </div>
      <nav className={styles.resources} aria-label="Resources">
        <a href="https://developers.openai.com/codex/cli" target="_blank" rel="noopener noreferrer">
          <AgentIcon agent="codex" size={14} /> Codex docs
        </a>
        <a
          href="https://code.claude.com/docs/en/overview"
          target="_blank"
          rel="noopener noreferrer"
        >
          <AgentIcon agent="claude" size={14} /> Claude docs
        </a>
        <a href="https://github.com/rbee3u/aibox" target="_blank" rel="noopener noreferrer">
          <GitFork size={14} data-icon="github" aria-hidden="true" /> GitHub
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
  );
}

export function RequestsPage({ api: providedApi, standalone = true }: RequestsPageProps) {
  const api = useMemo(() => providedApi ?? createRequestApi(), [providedApi]);
  const { dismissNotification, notifications, reportFailure, resolveFailure } =
    useFailureNotifications();
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
  const [dialog, setDialog] = useState<Dialog>(null);
  const [deletion, setDeletion] = useState<Deletion>(null);
  const [focusAfterDelete, setFocusAfterDelete] = useState<string | null | undefined>(undefined);
  const [focusAfterInspection, setFocusAfterInspection] = useState<string | null | undefined>(
    undefined,
  );
  const [detailOpen, setDetailOpen] = useState(false);
  const listController = useRef<AbortController | null>(null);
  const deletionInProgress = useRef(false);
  const pageNavigation = useRef(false);
  const deleting = deletion?.kind === "batch";
  const deletingRecordId = deletion?.kind === "record" ? deletion.id : null;
  const deletionBusy = deletion !== null;
  const dialogOpen = dialog !== null;

  const handleInspectionFailure = useCallback(
    (failure: InspectionFailure) => {
      const title =
        failure.kind === "detail"
          ? "Couldn’t load record"
          : failure.kind === "body"
            ? "Couldn’t load Body"
            : "Couldn’t download Body";
      reportFailure("inspection", title, failure.message, failure.retryable !== false);
    },
    [reportFailure],
  );
  const handleInspectionRecovery = useCallback(
    () => resolveFailure("inspection"),
    [resolveFailure],
  );
  const inspection = useRecordInspection({
    api,
    paused: dialogOpen,
    onFailure: handleInspectionFailure,
    onRecovery: handleInspectionRecovery,
  });
  const {
    bodies,
    bodyStatus,
    clearCurrentRecord,
    clearRecordIfCurrent,
    currentId,
    decodedBodies,
    detail,
    download,
    eventTimings,
    loadingBody,
    loadingDetail,
    retryFailure: retryInspectionFailure,
    selectRecord,
    setTab,
    tab,
  } = inspection;

  const openRecord = useCallback(
    (id: string) => {
      setFocusAfterInspection(undefined);
      setDetailOpen(true);
      void selectRecord(id);
    },
    [selectRecord],
  );

  const returnToList = useCallback(() => {
    setFocusAfterInspection(currentId);
    setDetailOpen(false);
    clearCurrentRecord();
  }, [clearCurrentRecord, currentId]);

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
        resolveFailure("list");
        return payload;
      } catch (cause) {
        if (listController.current === controller && !requestWasCancelled(cause, controller.signal))
          reportFailure("list", "Couldn’t load request records", cause, true);
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
    [api, reportFailure, resolveFailure],
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
    if (selectionMode || dialogOpen) {
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
  }, [dialogOpen, refreshWithFallback, selectionMode]);

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
    resolveFailure("action");
    const targetPage = dialog.ids.reduce(
      (minimum, id) => Math.min(minimum, selectionPages.current.get(id) ?? pageRef.current),
      Number.MAX_SAFE_INTEGER,
    );
    try {
      const deletedCount = await api.deleteRecords(dialog.ids);
      const deletedIds = dialog.ids;
      setList((current) =>
        removeDeletedFromList(current, deletedIds, deletedCount, pageRef.current),
      );
      setSelected(new Set());
      selectionPages.current.clear();
      setSelectionMode(false);
      if (currentId && deletedIds.includes(currentId)) {
        clearCurrentRecord();
      }
      setDialog(null);
      resolveFailure("action");
      await refreshWithFallback(targetPage);
      setFocusAfterDelete(null);
    } catch (cause) {
      const title = dialog.ids.length === 1 ? "Couldn’t delete record" : "Couldn’t delete records";
      setDialog(null);
      reportFailure("action", title, cause);
    } finally {
      finishDeletion();
    }
  }

  async function deleteRecord(id: string) {
    if (!beginDeletion({ kind: "record", id })) return;
    const originPage = pageRef.current;
    const originRecords = list.records;
    resolveFailure("action");
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
      reportFailure("action", "Couldn’t delete record", cause);
    } finally {
      finishDeletion();
    }
  }

  function handleNotificationAction(notification: NotificationItemData) {
    resolveFailure(notification.source);
    if (notification.source === "list") {
      void refreshPage();
    } else if (notification.source === "inspection") {
      retryInspectionFailure();
    }
  }

  return (
    <div className={styles.app}>
      {standalone && <StandaloneHeader />}
      <main
        className={`${styles.main} ${detailOpen && currentId !== null ? styles.detailOpen : ""}`}
      >
        <div className={styles.listColumn}>
          <RecordList
            records={list.records}
            total={list.total}
            page={page}
            hasPrevious={page > 1}
            hasNext={list.has_next}
            selectionMode={selectionMode}
            selected={selected}
            currentId={currentId}
            onEnterSelection={() => {
              setDetailOpen(false);
              setSelectionMode(true);
            }}
            onExitSelection={exitSelectionMode}
            onTogglePage={togglePageSelection}
            onToggle={toggleRecordSelection}
            onSelect={openRecord}
            onPrevious={() => void loadPage(page - 1)}
            onNext={() => void loadPage(page + 1)}
            loading={loadingList}
            refreshing={refreshing}
            deletableCount={list.deletable_count}
            onRefresh={() => void refreshPage()}
            onDeleteSelected={() => setDialog({ ids: selectedIds })}
            onDeleteRecord={(id) => void deleteRecord(id)}
            deletingRecordId={deletingRecordId}
            deletionBusy={deletionBusy}
            focusAfterDelete={focusAfterDelete}
            onFocusAfterDelete={() => setFocusAfterDelete(undefined)}
            focusAfterInspection={focusAfterInspection}
            onFocusAfterInspection={() => setFocusAfterInspection(undefined)}
          />
        </div>
        <div className={styles.detailColumn}>
          {currentId && (
            <button
              type="button"
              className={styles.detailBack}
              aria-label="Back to Request Record list"
              title="Back to Request Record list"
              onClick={returnToList}
            >
              <ChevronLeft size={18} aria-hidden="true" />
            </button>
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
          ) : currentId ? (
            <section className={styles.emptyDetail}>
              <CircleAlert size={26} aria-hidden="true" />
              <h1>Record unavailable</h1>
              <p>Request Record details could not be loaded.</p>
            </section>
          ) : (
            <section className={styles.emptyDetail}>
              <ArrowLeftRight size={26} data-icon="request-detail-empty" aria-hidden="true" />
              <h1>Select a request</h1>
              <p>Choose a Request Record to inspect its summary and raw data.</p>
            </section>
          )}
        </div>
      </main>
      <NotificationCenter
        notifications={
          selectionMode
            ? notifications.map((notification) =>
                notification.source === "list"
                  ? { ...notification, actionLabel: undefined }
                  : notification,
              )
            : notifications
        }
        paused={dialogOpen}
        onAction={handleNotificationAction}
        onDismiss={dismissNotification}
      />
      {dialog && (
        <ConfirmDialog
          title={`Delete ${dialog.ids.length} selected record${dialog.ids.length === 1 ? "" : "s"}?`}
          message="This permanently deletes the selected raw request and response data."
          confirmLabel="Delete permanently"
          onConfirm={() => void confirmDelete()}
          onCancel={() => !deleting && setDialog(null)}
          busy={deleting}
        />
      )}
    </div>
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
