import {
  ArrowLeftRight,
  FileSliders,
  LayoutDashboard,
  MessagesSquare,
  UsersRound,
  type LucideIcon,
} from "lucide-react";

export type ModuleId = "overview" | "tenants" | "configs" | "sessions" | "requests";

export const moduleIcons: Record<ModuleId, LucideIcon> = {
  overview: LayoutDashboard,
  tenants: UsersRound,
  configs: FileSliders,
  sessions: MessagesSquare,
  requests: ArrowLeftRight,
};
