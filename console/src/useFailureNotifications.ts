import { useCallback, useRef, useState } from "react";
import { requestErrorMessage } from "./api";
import type { NotificationItemData } from "./components/NotificationCenter";

type NotificationSource = NotificationItemData["source"];

export function useFailureNotifications() {
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
      const message = typeof cause === "string" ? cause : requestErrorMessage(cause);
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
    [],
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
