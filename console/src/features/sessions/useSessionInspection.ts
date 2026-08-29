import { useCallback, useReducer, useRef, useState } from "react";
import type { SessionApi, SessionDetailMeta, SessionDetailStats } from "@/api/sessions";
import {
  appendActivityItem,
  appendConversationMessage,
  emptySessionDetail,
  sessionDetailReducer,
  type SessionActivityItem,
  type SessionTimelineItem,
} from "@/features/sessions/sessionDetail";
import type { SourcedSession } from "@/features/sessions/sessionSource";

function requestCancelled(cause: unknown, signal: AbortSignal): boolean {
  return signal.aborted || (cause instanceof DOMException && cause.name === "AbortError");
}

export function useSessionInspection(
  api: SessionApi,
  onFailure: (session: SourcedSession, cause: unknown) => void,
) {
  const [currentSession, setCurrentSession] = useState<SourcedSession | null>(null);
  const [detailState, dispatchDetail] = useReducer(sessionDetailReducer, emptySessionDetail);
  const [detailRevision, setDetailRevision] = useState(0);
  const streamController = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<SourcedSession | null>(null);

  const abort = useCallback(() => {
    streamController.current?.abort();
    streamController.current = null;
    dispatchDetail({ type: "stop" });
  }, []);

  const clear = useCallback(() => {
    abort();
    currentSessionRef.current = null;
    setCurrentSession(null);
    dispatchDetail({ type: "reset" });
  }, [abort]);

  const inspect = useCallback(
    async (row: SourcedSession, preserveContent = false) => {
      abort();
      const controller = new AbortController();
      streamController.current = controller;
      currentSessionRef.current = row;
      setCurrentSession(row);
      setDetailRevision((current) => current + 1);
      dispatchDetail({ type: "start", preserveContent });
      let nextTimeline: SessionTimelineItem[] = [];
      let nextMeta: SessionDetailMeta | null = null;
      let nextStats: SessionDetailStats | null = null;
      let nextWarnings: string[] = [];
      try {
        await api.streamSessionDetail(
          row.source.tenant,
          row.source.agent,
          row.id,
          {
            onMeta: (meta) => {
              if (preserveContent) nextMeta = meta;
              else dispatchDetail({ type: "meta", value: meta });
            },
            onMessage: (message) => {
              if (preserveContent) nextTimeline = appendConversationMessage(nextTimeline, message);
              else dispatchDetail({ type: "message", value: message });
            },
            onTool: (tool) => {
              const entry: SessionActivityItem = { kind: "tool", value: tool };
              if (preserveContent) nextTimeline = appendActivityItem(nextTimeline, entry);
              else dispatchDetail({ type: "activity", value: entry });
            },
            onEvidence: (evidence) => {
              const entry: SessionActivityItem = { kind: "evidence", value: evidence };
              if (preserveContent) nextTimeline = appendActivityItem(nextTimeline, entry);
              else dispatchDetail({ type: "activity", value: entry });
            },
            onComplete: (stats, warnings) => {
              if (preserveContent) {
                nextStats = stats;
                nextWarnings = warnings;
              } else {
                dispatchDetail({ type: "complete", stats, warnings });
              }
            },
          },
          controller.signal,
        );
        if (preserveContent && streamController.current === controller) {
          dispatchDetail({
            type: "replace",
            timeline: nextTimeline,
            meta: nextMeta,
            stats: nextStats,
            warnings: nextWarnings,
          });
        }
      } catch (cause) {
        if (!requestCancelled(cause, controller.signal)) onFailure(row, cause);
      } finally {
        if (streamController.current === controller) {
          streamController.current = null;
          dispatchDetail({ type: "stop" });
        }
      }
    },
    [abort, api, onFailure],
  );

  const inspectedSession = useCallback(() => currentSessionRef.current, []);

  const replaceCurrent = useCallback((row: SourcedSession) => {
    currentSessionRef.current = row;
    setCurrentSession(row);
  }, []);

  return {
    abort,
    clear,
    currentSession,
    detailRevision,
    detailState,
    inspect,
    inspectedSession,
    replaceCurrent,
  };
}
