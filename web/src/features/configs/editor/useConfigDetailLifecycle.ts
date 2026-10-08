import { useEffect, useRef } from "react";
import type { ConfigListData } from "@/api/configs";
import type { AgentKind } from "@/domain/agent";
import type { TenantSelection, TenantSelectionValue } from "@/domain/tenant";
import {
  configLocation,
  namedConfigName,
  type ConfigSelection,
} from "@/features/common/routes/configs";
import type { ConfigPendingAction } from "@/features/configs/configWorkflow";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { useNarrowDetailFocus } from "@/shared/hooks/useNarrowDetailFocus";
import type { ModuleLocationChange } from "@/shared/lib/navigation";

interface ConfigDetailLifecycleOptions {
  agent: AgentKind;
  selection: ConfigSelection;
  tenant: TenantSelection;
  detailOpen: boolean;
  selectedConfigKey: string;
  selectedTenantSelectionValue: TenantSelectionValue;
  file: string | null;
  catalog: ConfigListData | null;
  focusConfigRow: (key: string) => boolean;
  requestEditorAction: (
    action: () => void | Promise<void>,
    kind?: ConfigPendingAction["kind"],
  ) => void;
  resetSelection: () => void;
  onLocationChange: ModuleLocationChange;
}

/** Owns route-driven file reveal and detail focus; the editor owns drafts and mode. */
export function useConfigDetailLifecycle({
  agent,
  selection,
  tenant,
  detailOpen,
  selectedConfigKey,
  selectedTenantSelectionValue,
  file,
  catalog,
  focusConfigRow,
  requestEditorAction,
  resetSelection,
  onLocationChange,
}: ConfigDetailLifecycleOptions) {
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const detailBackButtonRef = useRef<HTMLButtonElement>(null);
  const focusAfterDetailClose = useRef<string | null>(null);
  useNarrowDetailFocus(detailBackButtonRef, detailOpen, file, selectedConfigKey);
  const panes = useElementRegistry<HTMLDivElement>();
  function closeConfigDetail() {
    requestEditorAction(() => {
      focusAfterDetailClose.current = selection.current
        ? "current"
        : (namedConfigName(selection) ?? "named-catalog");
      onLocationChange(configLocation(tenant, agent, null));
    });
  }
  useEffect(() => {
    if (detailOpen || !focusAfterDetailClose.current) return;
    const key = focusAfterDetailClose.current;
    let focusFrame = 0;
    const revealFrame = window.requestAnimationFrame(() => {
      focusFrame = window.requestAnimationFrame(() => {
        if (focusConfigRow(key)) focusAfterDetailClose.current = null;
      });
    });
    return () => {
      window.cancelAnimationFrame(revealFrame);
      window.cancelAnimationFrame(focusFrame);
    };
  }, [catalog, focusConfigRow, detailOpen]);
  useEffect(() => {
    if (!detailOpen || !file) return;
    const frame = window.requestAnimationFrame(() =>
      panes.get(file)?.scrollIntoView?.({ block: "nearest" }),
    );
    return () => window.cancelAnimationFrame(frame);
  }, [detailOpen, file, catalog, panes]);
  useEffect(() => {
    resetSelection();
  }, [agent, resetSelection, selectedTenantSelectionValue]);
  return {
    closeConfigDetail,
    detailBackButtonRef,
    detailHeadingRef,
    registerPane: panes.register,
  };
}
