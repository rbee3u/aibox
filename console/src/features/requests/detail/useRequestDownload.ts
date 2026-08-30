import { useCallback, useEffect, useRef } from "react";
import type { BodyKind, RequestsApi } from "@/api/requests";
import type {
  ClearInspectionFailure,
  ReportInspectionFailure,
  RequestInspectionIdentity,
} from "@/features/requests/detail/inspectionTypes";
import { requestErrorMessage, requestWasCancelled } from "@/features/requests/requestErrors";

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
  const controllerRef = useRef<AbortController | null>(null);

  useEffect(() => {
    controllerRef.current?.abort();
    controllerRef.current = null;
  }, [identity?.generation, paused]);

  useEffect(() => () => controllerRef.current?.abort(), []);

  return useCallback(
    async (kind: BodyKind) => {
      const selected = identityRef.current;
      if (!selected || paused) return;
      controllerRef.current?.abort();
      const controller = new AbortController();
      controllerRef.current = controller;
      clearFailure("download");
      try {
        const { bytes: data } = await api.loadBody(selected.id, kind, 0, controller.signal);
        if (
          controller.signal.aborted ||
          controllerRef.current !== controller ||
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
        if (
          controllerRef.current === controller &&
          !requestWasCancelled(cause, controller.signal)
        ) {
          reportFailure({
            kind: "download",
            message: requestErrorMessage(cause),
            bodyKind: kind,
          });
        }
      } finally {
        if (controllerRef.current === controller) controllerRef.current = null;
      }
    },
    [api, clearFailure, paused, reportFailure],
  );
}
