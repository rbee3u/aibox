import type { LucideIcon } from "lucide-react";
import { moduleIcons } from "@/shared/icons/consoleIcons";
import type { ModuleId } from "@/shared/lib/navigation";

export const CONSOLE_BASE_PATH = "/_aibox/ui";

export interface ConsoleModule {
  id: ModuleId;
  label: string;
  detail: string;
  icon: LucideIcon;
}

export const consoleModules: readonly ConsoleModule[] = [
  { id: "overview", label: "Overview", detail: "Service and topology", icon: moduleIcons.overview },
  { id: "tenants", label: "Tenants", detail: "Tenant Components", icon: moduleIcons.tenants },
  {
    id: "configs",
    label: "Configs",
    detail: "Current and Named Configs",
    icon: moduleIcons.configs,
  },
  {
    id: "sessions",
    label: "Sessions",
    detail: "Coding Agent transcripts",
    icon: moduleIcons.sessions,
  },
  {
    id: "requests",
    label: "Requests",
    detail: "Request diagnostics",
    icon: moduleIcons.requests,
  },
];

export const DEFAULT_MODULE: ModuleId = "overview";

export function moduleFromPath(pathname: string): ModuleId {
  const value = pathname.split("/").filter(Boolean).at(-1);
  return consoleModules.some((module) => module.id === value)
    ? (value as ModuleId)
    : DEFAULT_MODULE;
}

export function modulePath(module: ModuleId, query?: URLSearchParams): string {
  const suffix = query?.toString();
  return `${CONSOLE_BASE_PATH}/${module}${suffix ? `?${suffix}` : ""}`;
}

export function moduleById(module: ModuleId): ConsoleModule {
  const found = consoleModules.find((candidate) => candidate.id === module);
  if (!found) throw new Error(`unknown Console module ${module}`);
  return found;
}
