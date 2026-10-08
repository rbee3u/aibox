import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import type { ConfigApi, ConfigFileTarget } from "@/api/configs";
import { ConfigEditorSession } from "@/features/configs/editor/configEditorSession";

interface Options {
  api: ConfigApi;
  target: Omit<ConfigFileTarget, "file">;
  files: readonly string[];
  enabled: boolean;
  onDirtyChange?: (dirty: boolean) => void;
  onError: (message: string | null) => void;
  onSaved: () => void;
  leave: { pending?: boolean; onCancel?: () => void; onContinue?: () => void | Promise<void> };
}

/** React owns the lifetime; the selected Config owns all editable file state. */
export function useConfigEditorSession({
  api,
  target,
  files,
  enabled,
  onDirtyChange,
  onError,
  onSaved,
  leave,
}: Options) {
  const targetKey = JSON.stringify(target);
  const filesKey = JSON.stringify(files);
  const session = useMemo(
    () =>
      new ConfigEditorSession(
        api,
        JSON.parse(targetKey) as Omit<ConfigFileTarget, "file">,
        JSON.parse(filesKey) as string[],
      ),
    [api, targetKey, filesKey],
  );
  useEffect(() => {
    session.setCallbacks(onError, onSaved);
  }, [session, onError, onSaved]);
  const state = useSyncExternalStore(session.subscribe, session.getSnapshot);
  useEffect(() => {
    if (enabled) session.start();
    return () => session.stop();
  }, [session, enabled]);
  const dirty = session.dirtyFiles.length > 0;
  useEffect(() => onDirtyChange?.(dirty), [dirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);
  const leaveArmed = useRef(false);
  const { pending, onContinue, onCancel } = leave;
  useEffect(() => {
    if (!pending || !onContinue) {
      leaveArmed.current = false;
      return;
    }
    if (leaveArmed.current) return;
    leaveArmed.current = true;
    session.deferLeave(onContinue);
  }, [session, pending, onContinue]);
  return {
    session,
    state,
    cancelPending: () => {
      session.cancelPending();
      if (pending) onCancel?.();
    },
  };
}
