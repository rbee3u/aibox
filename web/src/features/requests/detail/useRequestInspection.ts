import { useCallback, useRef, useState } from "react";
import type { RequestsApi } from "@/api/requests";
import type { InspectionFailure } from "@/features/requests/detail/inspectionTypes";
import { useRequestBodyResource } from "@/features/requests/detail/useRequestBodyResource";
import { useRequestDetailResource } from "@/features/requests/detail/useRequestDetailResource";
import { useRequestDownload } from "@/features/requests/detail/useRequestDownload";
import type { DetailTab } from "@/features/requests/viewTypes";

export type { InspectionFailure } from "@/features/requests/detail/inspectionTypes";

interface UseRequestInspectionOptions {
  api: RequestsApi;
  initialTab?: DetailTab;
  paused: boolean;
  onFailure: (failure: InspectionFailure) => void;
  onRecovery: () => void;
}

export function useRequestInspection({
  api,
  initialTab = "summary",
  paused,
  onFailure,
  onRecovery,
}: UseRequestInspectionOptions) {
  const [failure, setFailure] = useState<InspectionFailure | null>(null);
  const failureRef = useRef<InspectionFailure | null>(null);

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

  const {
    clear: clearDetail,
    currentId,
    detail,
    identity,
    loading: loadingDetail,
    select: selectDetail,
    setTab: setDetailTab,
    tab,
  } = useRequestDetailResource({
    api,
    clearFailure,
    initialTab,
    paused,
    reportFailure,
  });

  const {
    bodies,
    bodyStatus,
    decodedBodies,
    eventTimings,
    loadingBody,
    reset: resetBody,
    retry: retryBody,
  } = useRequestBodyResource({
    api,
    clearFailure,
    detail,
    identity,
    loadingDetail,
    paused,
    reportFailure,
    tab,
  });
  const download = useRequestDownload({
    api,
    clearFailure,
    identity,
    paused,
    reportFailure,
  });

  const clearCurrentRequest = useCallback(() => {
    clearDetail();
    resetBody();
    clearFailure();
  }, [clearDetail, clearFailure, resetBody]);

  const selectRequest = useCallback(
    async (id: string, nextTab: DetailTab = "summary") => {
      resetBody();
      await selectDetail(id, nextTab);
    },
    [resetBody, selectDetail],
  );

  const clearRequestIfCurrent = useCallback(
    (id: string) => {
      if (currentId === id) clearCurrentRequest();
    },
    [clearCurrentRequest, currentId],
  );

  const retryFailure = useCallback(() => {
    if (!failure) return;
    const current = failure;
    clearFailure();
    if (current.kind === "detail") {
      if (currentId) void selectRequest(currentId, tab);
    } else if (current.kind === "body") {
      retryBody();
    } else if (current.bodyKind) {
      void download(current.bodyKind);
    }
  }, [clearFailure, currentId, download, failure, retryBody, selectRequest, tab]);

  const selectTab = useCallback(
    (value: DetailTab) => {
      clearFailure();
      setDetailTab(value);
    },
    [clearFailure, setDetailTab],
  );

  return {
    bodies,
    bodyStatus,
    clearCurrentRequest,
    clearRequestIfCurrent,
    currentId,
    decodedBodies,
    detail,
    download,
    failure,
    eventTimings,
    loadingBody,
    loadingDetail,
    retryFailure,
    selectRequest,
    setTab: selectTab,
    tab,
  };
}
