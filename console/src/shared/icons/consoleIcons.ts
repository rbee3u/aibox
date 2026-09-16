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

/**
 * The mark each status tone carries, so severity never rides on hue alone.
 *
 * `--warning` and `--danger` are a step apart in lightness and adjacent in
 * hue: simulated for deuteranopia they land 6 ΔE apart in the light theme,
 * which is the same colour. Any surface that renders both tones in one list
 * therefore has to differ in shape as well, and every surface that renders
 * one of them should use the same shape as the others so the shape is
 * learnable. A triangle warns, a circled exclamation is an error, a circled
 * check is healthy.
 */
export const toneIcons = {
  good: CircleCheck,
  warning: TriangleAlert,
  error: CircleAlert,
} as const satisfies Record<string, LucideIcon>;

export type IconTone = keyof typeof toneIcons;
