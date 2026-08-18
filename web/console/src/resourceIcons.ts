import {
  Box,
  Boxes,
  Container,
  FileClock,
  FileCode2,
  FileCog,
  House,
  Wrench,
  type LucideIcon,
} from "lucide-react";

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
  session: FileClock,
  components: Boxes,
  component: Wrench,
};
