import { useCallback, useEffect, useRef, useState } from "react";
import type { RefObject } from "react";
import type { RequestList, RequestsApi } from "@/api/requests";
import { REQUESTS_PER_PAGE } from "@/features/requests/listModel";

import { usePolling } from "@/shared/hooks/usePolling";
import { LatestRequest } from "@/shared/lib/latestRequest";
import { messageOf } from "@/shared/lib/errors";

const LIST_POLL_INTERVAL_MS = 5000;

const emptyList: RequestList = {
  requests: [],
  total: 0,
  deletable_count: 0,
  has_next: false,
};

interface RequestCatalogOptions {
  api: RequestsApi;
  initialPage: number;
  pageRef: RefObject<number>;
  deletionInProgress: RefObject<boolean>;
  enabled: boolean;
  onFallbackPage: (page: number) => void;
}

/** Paginated list loading, refresh fallback, cancellation, and polling. */
export function useRequestCatalog({
  api,
  initialPage,
  pageRef,
  deletionInProgress,
  enabled,
  onFallbackPage,
}: RequestCatalogOptions) {
  const [list, setList] = useState<RequestList>(emptyList);
  const [page, setPage] = useState(initialPage);
  const [loadingList, setLoadingList] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const listRequest = useRef(new LatestRequest());
  const apiOwner = useRef(api);
  const pageNavigation = useRef(false);
  const failedListPage = useRef<number | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const loadPage = useCallback(
    async (pageToLoad: number, background = false): Promise<RequestList | null> => {
      if (background && (pageNavigation.current || deletionInProgress.current)) return null;
      const targetPage = Math.max(1, pageToLoad);
      if (!background) {
        pageNavigation.current = true;
        setLoadingList(true);
      }
      return listRequest.current.run((signal) => api.listRequests(targetPage, signal), {
        loaded: (payload) => {
          setList(payload);
          setPage(targetPage);
          pageRef.current = targetPage;
          if (
            !background ||
            failedListPage.current === null ||
            failedListPage.current === targetPage
          ) {
            failedListPage.current = null;
            setListError(null);
          }
        },
        failed: (cause) => {
          if (cause instanceof DOMException && cause.name === "AbortError") return;
          if (!background || failedListPage.current === null) failedListPage.current = targetPage;
          setListError(messageOf(cause));
        },
        settled: () => {
          if (!background) {
            pageNavigation.current = false;
            setLoadingList(false);
          }
        },
      });
    },
    [api, deletionInProgress, pageRef],
  );

  useEffect(() => {
    if (apiOwner.current === api) return;
    apiOwner.current = api;
    void loadPage(pageRef.current);
  }, [api, loadPage, pageRef]);

  const refreshWithFallback = useCallback(
    async (targetPage = pageRef.current, background = false) => {
      let candidate = Math.max(1, targetPage);
      while (true) {
        const payload = await loadPage(candidate, background);
        if (!payload) return null;
        if (payload.requests.length > 0 || candidate === 1) return { page: candidate, payload };
        const lastPage = Math.max(1, Math.ceil(payload.total / REQUESTS_PER_PAGE));
        candidate = Math.min(candidate - 1, lastPage);
        onFallbackPage(candidate);
      }
    },
    [loadPage, onFallbackPage, pageRef],
  );

  const refreshPage = useCallback(async () => {
    setRefreshing(true);
    try {
      await refreshWithFallback(page);
    } finally {
      setRefreshing(false);
    }
  }, [page, refreshWithFallback]);

  const retryListFailure = useCallback(async () => {
    const targetPage = failedListPage.current ?? pageRef.current;
    setRefreshing(true);
    try {
      const refreshed = await refreshWithFallback(targetPage);
      if (!refreshed) return;
      onFallbackPage(refreshed.page);
    } finally {
      setRefreshing(false);
    }
  }, [refreshWithFallback, onFallbackPage, pageRef]);

  const cancelListRequest = useCallback(() => listRequest.current.cancel(), []);
  const pollList = useCallback(
    async (first: boolean) => {
      await refreshWithFallback(pageRef.current, !first);
    },
    [refreshWithFallback, pageRef],
  );
  usePolling({
    enabled,
    intervalMs: LIST_POLL_INTERVAL_MS,
    run: pollList,
    onCancel: cancelListRequest,
  });

  return {
    list,
    setList,
    page,
    loadingList,
    refreshing,
    listError,
    loadPage,
    failedListPage,
    refreshWithFallback,
    refreshPage,
    retryListFailure,
    cancelListRequest,
  };
}
