import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { FocusEvent } from "react";
import { ActionButton } from "./ActionButton";
import styles from "./NotificationCenter.module.css";

export type NotificationTone = "error" | "success" | "info";

export interface NotificationItemData {
  id: number;
  source: "list" | "inspection" | "action";
  tone: NotificationTone;
  title: string;
  message: string;
  actionLabel?: string;
}

interface NotificationCenterProps {
  notifications: NotificationItemData[];
  paused: boolean;
  onAction: (notification: NotificationItemData) => void;
  onDismiss: (source: NotificationItemData["source"]) => void;
}

const SUCCESS_DISPLAY_MS = 4000;
const DEFAULT_DISPLAY_MS = 8000;

export function NotificationCenter({
  notifications,
  paused,
  onAction,
  onDismiss,
}: NotificationCenterProps) {
  const [pageVisible, setPageVisible] = useState(document.visibilityState !== "hidden");
  const [windowFocused, setWindowFocused] = useState(true);

  useEffect(() => {
    const handleVisibility = () => setPageVisible(document.visibilityState !== "hidden");
    const handleFocus = () => setWindowFocused(true);
    const handleBlur = () => setWindowFocused(false);
    document.addEventListener("visibilitychange", handleVisibility);
    window.addEventListener("focus", handleFocus);
    window.addEventListener("blur", handleBlur);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibility);
      window.removeEventListener("focus", handleFocus);
      window.removeEventListener("blur", handleBlur);
    };
  }, []);

  if (notifications.length === 0) return null;

  return (
    <section className={styles.center} aria-label="Notifications">
      {[...notifications]
        .sort((left, right) => right.id - left.id)
        .map((notification) => (
          <NotificationItem
            key={notification.source}
            notification={notification}
            paused={paused || !pageVisible || !windowFocused}
            onAction={onAction}
            onDismiss={onDismiss}
          />
        ))}
    </section>
  );
}

interface NotificationItemProps {
  notification: NotificationItemData;
  paused: boolean;
  onAction: (notification: NotificationItemData) => void;
  onDismiss: (source: NotificationItemData["source"]) => void;
}

function NotificationItem({ notification, paused, onAction, onDismiss }: NotificationItemProps) {
  const [interacting, setInteracting] = useState(false);
  const displayMs = notification.tone === "success" ? SUCCESS_DISPLAY_MS : DEFAULT_DISPLAY_MS;
  const remaining = useRef(displayMs);
  const startedAt = useRef(0);
  const dismiss = useCallback(
    () => onDismiss(notification.source),
    [notification.source, onDismiss],
  );

  useEffect(() => {
    remaining.current = displayMs;
  }, [displayMs, notification.id]);

  useEffect(() => {
    if (paused || interacting) return;
    startedAt.current = Date.now();
    const timer = window.setTimeout(dismiss, remaining.current);
    return () => {
      window.clearTimeout(timer);
      remaining.current = Math.max(0, remaining.current - (Date.now() - startedAt.current));
    };
  }, [dismiss, interacting, notification.id, paused]);

  function handleBlur(event: FocusEvent<HTMLElement>) {
    if (!event.currentTarget.contains(event.relatedTarget)) setInteracting(false);
  }

  const Icon =
    notification.tone === "error"
      ? AlertCircle
      : notification.tone === "success"
        ? CheckCircle2
        : Info;

  return (
    <article
      className={`${styles.notification} ${styles[notification.tone]}`}
      role={notification.tone === "error" ? "alert" : "status"}
      onMouseEnter={() => setInteracting(true)}
      onMouseLeave={() => setInteracting(false)}
      onFocusCapture={() => setInteracting(true)}
      onBlurCapture={handleBlur}
    >
      <Icon className={styles.icon} size={17} aria-hidden="true" />
      <div className={styles.copy}>
        <strong>{notification.title}</strong>
        <span>{notification.message}</span>
      </div>
      {notification.actionLabel && (
        <ActionButton className={styles.action} tone="quiet" onClick={() => onAction(notification)}>
          {notification.actionLabel}
        </ActionButton>
      )}
      <ActionButton
        className={styles.dismiss}
        tone="quiet"
        aria-label="Dismiss message"
        onClick={dismiss}
      >
        <X size={15} aria-hidden="true" />
      </ActionButton>
    </article>
  );
}
