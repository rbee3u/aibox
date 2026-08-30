import { useCallback, useEffect, useRef, useState } from "react";
import type { ConfigFileController } from "@/features/configs/detail/configFileController";
import type { ConfigPendingAction } from "@/features/configs/route";

export interface ConfigFileStatus {
  dirty: boolean;
  canSave: boolean;
}

export function useConfigEditorSession(
  files: readonly string[],
  scopeKey: string,
  onDirtyChange: ((dirty: boolean) => void) | undefined,
  onError: (message: string | null) => void,
) {
  const controllers = useRef(new Map<string, ConfigFileController>());
  const revealRetries = useRef(new Map<string, () => void>());
  const [fileStatuses, setFileStatuses] = useState<Record<string, ConfigFileStatus>>({});
  const [pendingAction, setPendingAction] = useState<ConfigPendingAction | null>(null);
  const editorDirty = Object.values(fileStatuses).some((status) => status.dirty);
  const dirtyFiles = files.filter((name) => fileStatuses[name]?.dirty);

  useEffect(() => onDirtyChange?.(editorDirty), [editorDirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);

  useEffect(() => {
    controllers.current.clear();
    revealRetries.current.clear();
    // One Tenant-and-Agent selection owns one editor session registry.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setFileStatuses({});
  }, [scopeKey]);

  const registerController = useCallback(
    (name: string, controller: ConfigFileController | null) => {
      setFileStatuses((current) => {
        const next = { ...current };
        if (controller) {
          controllers.current.set(name, controller);
          next[name] = { dirty: controller.dirty, canSave: controller.canSave };
        } else {
          controllers.current.delete(name);
          delete next[name];
        }
        return next;
      });
    },
    [],
  );

  const registerRevealRetry = useCallback((name: string, retry: (() => void) | null) => {
    if (retry) revealRetries.current.set(name, retry);
    else revealRetries.current.delete(name);
  }, []);

  const retryReveals = useCallback(() => {
    for (const retry of revealRetries.current.values()) retry();
  }, []);

  const prepareMainConfigSave = useCallback(
    (customProvider: boolean) => {
      if (!customProvider) return true;
      const auth = controllers.current.get("auth.json");
      if (!auth?.dirty) return true;
      onError("Save auth.json before saving a Custom provider configuration.");
      return false;
    },
    [onError],
  );

  const reloadFile = useCallback((name: string) => {
    controllers.current.get(name)?.reload();
  }, []);

  const reloadFiles = useCallback((names: readonly string[]) => {
    for (const name of names) controllers.current.get(name)?.reload();
  }, []);

  const requestAction = useCallback(
    (run: () => void | Promise<void>) => {
      if (editorDirty) setPendingAction({ run });
      else void run();
    },
    [editorDirty],
  );

  const saveInOrder = useCallback(async (names: readonly string[]): Promise<boolean> => {
    for (const name of names) {
      const controller = controllers.current.get(name);
      if (controller?.dirty && !(await controller.save())) return false;
    }
    return true;
  }, []);

  const savePending = useCallback(
    async (names: readonly string[]) => {
      if (!pendingAction) return;
      const action = pendingAction.run;
      if (!(await saveInOrder(names))) return;
      setPendingAction(null);
      await action();
    },
    [pendingAction, saveInOrder],
  );

  const discardPending = useCallback(async () => {
    if (!pendingAction) return;
    const action = pendingAction.run;
    for (const controller of controllers.current.values()) {
      if (controller.dirty) controller.restore();
    }
    setPendingAction(null);
    await action();
  }, [pendingAction]);

  const cancelPending = useCallback(() => setPendingAction(null), []);

  return {
    cancelPending,
    dirtyFiles,
    discardPending,
    editorDirty,
    fileStatuses,
    pendingAction,
    prepareMainConfigSave,
    registerController,
    registerRevealRetry,
    reloadFile,
    reloadFiles,
    requestAction,
    retryReveals,
    saveInOrder,
    savePending,
  };
}
