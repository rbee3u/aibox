import { useCallback, useEffect, useRef } from "react";

import type { BodyKind, RequestsApi } from "@/api/requests";
import { requestErrorMessage } from "@/features/requests/requestErrors";
import type {
  ClearInspectionFailure,
  ReportInspectionFailure,
  RequestInspectionIdentity,
} from "@/features/requests/viewTypes";
import { LatestRequest } from "@/shared/lib/latestRequest";
import { wasCancelled } from "@/shared/lib/errors";

interface DownloadOptions {
  api: RequestsApi;
  clearFailure: ClearInspectionFailure;
  identity: RequestInspectionIdentity | null;
  paused: boolean;
  reportFailure: ReportInspectionFailure;
}

export function useRequestDownload({
  api,
  clearFailure,
  identity,
  paused,
  reportFailure,
}: DownloadOptions) {
  const identityRef = useRef(identity);
  useEffect(() => {
    identityRef.current = identity;
  }, [identity]);
  const requestOwner = useRef(new LatestRequest());

  useEffect(() => {
    requestOwner.current.cancel();
  }, [identity?.generation, paused]);

  useEffect(() => {
    const owner = requestOwner.current;
    return () => owner.cancel();
  }, []);

  return useCallback(
    async (kind: BodyKind) => {
      const selected = identityRef.current;
      if (!selected || paused) return;
      const request = requestOwner.current.begin();
      clearFailure("download");
      try {
        const { bytes: data } = await api.loadBody(selected.id, kind, 0, request.signal);
        if (
          request.signal.aborted ||
          !request.isCurrent() ||
          identityRef.current?.generation !== selected.generation
        ) {
          return;
        }
        const bodyBuffer = data.buffer.slice(
          data.byteOffset,
          data.byteOffset + data.byteLength,
        ) as ArrayBuffer;
        const url = URL.createObjectURL(new Blob([bodyBuffer]));
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = `${selected.id}.${kind}.body`;
        anchor.click();
        window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      } catch (cause) {
        if (request.isCurrent() && !wasCancelled(cause, request.signal)) {
          reportFailure({
            kind: "download",
            message: requestErrorMessage(cause),
            bodyKind: kind,
          });
        }
      } finally {
        request.release();
      }
    },
    [api, clearFailure, paused, reportFailure],
  );
}
