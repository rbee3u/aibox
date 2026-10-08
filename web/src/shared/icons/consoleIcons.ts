import {
  ArrowLeftRight,
  Box,
  Boxes,
  CircleAlert,
  CircleCheck,
  Container,
  FileCode2,
  FileCog,
  FileSliders,
  House,
  LayoutDashboard,
  MessagesSquare,
  TriangleAlert,
  UsersRound,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import type { ModuleId } from "@/shared/lib/navigation";

/** Canonical icon vocabulary shared by navigation, topology, lists, and detail views. */
export const moduleIcons: Record<ModuleId, LucideIcon> = {
  overview: LayoutDashboard,
  tenants: UsersRound,
  configs: FileSliders,
  sessions: MessagesSquare,
  requests: ArrowLeftRight,
};

export type ResourceIcon =
  | "service"
  | "hostTenant"
  | "managedTenant"
  | "currentConfig"
  | "namedConfig"
  | "session"
  | "components"
  | "component";

export const resourceIcons: Record<ResourceIcon, LucideIcon> = {
  service: Box,
  hostTenant: House,
  managedTenant: Container,
  currentConfig: FileCog,
  namedConfig: FileCode2,
  session: MessagesSquare,
  components: Boxes,
  component: Wrench,
};

/** Keep status shapes consistent so severity does not depend on color. */
export const toneIcons = {
  good: CircleCheck,
  warning: TriangleAlert,
  error: CircleAlert,
} as const satisfies Record<string, LucideIcon>;
