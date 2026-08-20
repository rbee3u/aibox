import {
  ArrowLeftRight,
  Box,
  Boxes,
  Container,
  FileCode2,
  FileCog,
  FileSliders,
  House,
  LayoutDashboard,
  MessagesSquare,
  UsersRound,
  Wrench,
  type LucideIcon,
} from "lucide-react";

export type ModuleId = "overview" | "tenants" | "configs" | "sessions" | "requests";

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
