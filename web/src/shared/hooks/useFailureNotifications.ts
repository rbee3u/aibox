import { useCallback, useRef, useState } from "react";
import type { NotificationItemData, NotificationSource } from "@/shared/ui/notificationTypes";
import { messageOrFallback } from "@/shared/lib/errors";

// Every current caller shows this wording for a rejection without a message.
const DEFAULT_FALLBACK_MESSAGE = "Requests API call failed";

/**
 * Collects failure notices for one page. Each source keeps at most one notice,
 * repeated identical failures stay silent until the source recovers, and the
 * stack retains only the newest three notices.
 */
export function useFailureNotifications(fallbackMessage = DEFAULT_FALLBACK_MESSAGE) {
  const [notifications, setNotifications] = useState<NotificationItemData[]>([]);
  const failureSignatures = useRef<Map<NotificationSource, string>>(new Map());
  const notificationSequence = useRef(0);

  const removeNotification = useCallback((source: NotificationSource) => {
    setNotifications((current) =>
      current.some((item) => item.source === source)
        ? current.filter((item) => item.source !== source)
        : current,
    );
  }, []);

  const reportFailure = useCallback(
    (source: NotificationSource, title: string, cause: unknown, retry = false) => {
      const message = typeof cause === "string" ? cause : messageOrFallback(cause, fallbackMessage);
      const signature = `${title}\n${message}`;
      if (failureSignatures.current.get(source) === signature) return;

      failureSignatures.current.set(source, signature);
      notificationSequence.current += 1;
      const notification: NotificationItemData = {
        id: notificationSequence.current,
        source,
        tone: "error",
        title,
        message,
        actionLabel: retry ? "Retry" : undefined,
      };
      setNotifications((current) =>
        [...current.filter((item) => item.source !== source), notification].slice(-3),
      );
    },
    [fallbackMessage],
  );

  const resolveFailure = useCallback(
    (source: NotificationSource) => {
      failureSignatures.current.delete(source);
      removeNotification(source);
    },
    [removeNotification],
  );

  return {
    dismissNotification: removeNotification,
    notifications,
    reportFailure,
    resolveFailure,
  };
}
