export type NotificationTone = "error" | "success" | "info";

/** Identifies which page activity produced a notice, so each keeps one slot. */
export type NotificationSource = "list" | "inspection" | "action";

export interface NotificationItemData {
  id: number;
  source: NotificationSource;
  tone: NotificationTone;
  title: string;
  message: string;
  actionLabel?: string;
}
