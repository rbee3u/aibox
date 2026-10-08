import type {
  ConfigApi,
  ConfigAuthData,
  ConfigComparisonInput,
  ConfigCustomProvider,
  ConfigFileData,
  ConfigFileTarget,
  ConfigVisualOption,
} from "@/api/configs";
import type { ConfigPendingAction } from "@/features/configs/configWorkflow";
import { requestProxyRoute } from "@/features/configs/configCatalog";
import {
  configEditorBytes,
  configFileCanSave,
  configFileDirty,
  configFileInput,
  configFileSnapshotModel,
  omitUnavailableOptions,
  visualSaveFailure,
  type SaveFailure,
} from "@/features/configs/editor/configFileModel";
import { encodeBase64 } from "@/shared/lib/encoding";
import { messageOf } from "@/shared/lib/errors";

export interface RawDiagnostic {
  message: string;
  line: number;
  column: number;
}
export interface ConfigFileState {
  snapshot: ConfigFileData | null;
  editor: string;
  textEditable: boolean;
  visualOptions: ConfigVisualOption[] | null;
  customProvider: ConfigCustomProvider | null;
  authMode: ConfigAuthData["mode"];
  authKey: string;
  loading: boolean;
  readError: string | null;
  rawDiagnostics: RawDiagnostic[];
  feedback: "idle" | "saving" | "saved";
  saveFailure: SaveFailure | null;
}
interface EditorSnapshot {
  files: Record<string, ConfigFileState>;
  revision: number;
  mode: "visual" | "raw";
  visualAvailable: boolean;
  pendingAction: ConfigPendingAction | null;
}
function emptyFile(): ConfigFileState {
  return {
    snapshot: null,
    editor: "",
    textEditable: true,
    visualOptions: null,
    customProvider: null,
    authMode: "api-key",
    authKey: "",
    loading: true,
    readError: null,
    rawDiagnostics: [],
    feedback: "idle",
    saveFailure: null,
  };
}

/** One selected Config owns its files independently of which editor views mount.
 * Immutable snapshots serve React; commands read the same snapshot synchronously,
 * so sequential saves never depend on a child registration or a later render.
 */
export class ConfigEditorSession {
  private snapshot: EditorSnapshot;
  private listeners = new Set<() => void>();
  private active = false;
  private epoch = 0;
  private visualInitialized = false;
  private loads = new Map<string, number>();
  private diagnoses = new Map<string, number>();
  private timers = new Map<string, ReturnType<typeof setTimeout>>();

  constructor(
    private api: Pick<
      ConfigApi,
      "bootstrap" | "revealConfigFile" | "diagnoseConfigFile" | "saveConfigFile"
    >,
    readonly target: Omit<ConfigFileTarget, "file">,
    readonly names: readonly string[],
    private onError: (message: string | null) => void = () => {},
    private onSaved: () => void = () => {},
  ) {
    this.snapshot = {
      files: Object.fromEntries(names.map((name) => [name, emptyFile()])),
      revision: 0,
      mode: "raw",
      visualAvailable: false,
      pendingAction: null,
    };
  }
  setCallbacks(onError: (message: string | null) => void, onSaved: () => void) {
    this.onError = onError;
    this.onSaved = onSaved;
  }
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  getSnapshot = () => this.snapshot;
  private commit(patch: Partial<EditorSnapshot>) {
    const changed =
      (patch.mode !== undefined && patch.mode !== this.snapshot.mode) ||
      (patch.files !== undefined &&
        this.names.some((name) => {
          const before = this.snapshot.files[name];
          const after = patch.files![name];
          return (
            before.snapshot !== after.snapshot ||
            before.editor !== after.editor ||
            before.visualOptions !== after.visualOptions ||
            before.customProvider !== after.customProvider ||
            before.authMode !== after.authMode ||
            before.authKey !== after.authKey
          );
        }));
    this.snapshot = {
      ...this.snapshot,
      ...patch,
      revision: this.snapshot.revision + (changed ? 1 : 0),
    };
    for (const listener of this.listeners) listener();
  }
  private patch(name: string, patch: Partial<ConfigFileState>) {
    this.commit({
      files: { ...this.snapshot.files, [name]: { ...this.snapshot.files[name], ...patch } },
    });
  }
  private clearTimer(key: string) {
    clearTimeout(this.timers.get(key));
    this.timers.delete(key);
  }
  private later(key: string, delay: number, run: () => void) {
    this.clearTimer(key);
    this.timers.set(
      key,
      setTimeout(() => {
        this.timers.delete(key);
        run();
      }, delay),
    );
  }
  start() {
    this.active = true;
    this.reloadFiles(this.names);
  }
  stop() {
    this.active = false;
    this.epoch++;
    for (const key of this.timers.keys()) this.clearTimer(key);
  }
  file(name: string) {
    const state = this.snapshot.files[name];
    const isAuth = name === "auth.json";
    const editorBytes = configEditorBytes(state.snapshot, state.textEditable, state.editor);
    const mode = this.snapshot.mode;
    return {
      ...state,
      isAuth,
      editorBytes,
      dirty: configFileDirty({ ...state, mode, isAuth, editorBytes }),
      canSave: configFileCanSave(state.snapshot, state.textEditable, isAuth, state.authMode, mode),
    };
  }
  get dirtyFiles() {
    return this.names.filter((name) => this.file(name).dirty);
  }
  get comparisonInputs(): ConfigComparisonInput[] {
    return this.names.flatMap((file) => {
      const data = this.file(file);
      if (!data.snapshot || !data.editorBytes) return [];
      return [
        {
          ...configFileInput({
            ...data,
            snapshot: data.snapshot,
            editorBytes: data.editorBytes,
            mode: data.isAuth && data.authMode === "chatgpt" ? "raw" : this.snapshot.mode,
          }),
          file,
          originalBase64: data.snapshot.content_base64,
        },
      ];
    });
  }
  private accept(name: string, value: ConfigFileData) {
    this.diagnoses.set(name, (this.diagnoses.get(name) ?? 0) + 1);
    this.clearTimer(`diagnose:${name}`);
    const model = configFileSnapshotModel(
      value,
      this.target.tenant,
      this.api.bootstrap?.listen,
      name === "auth.json",
    );
    this.patch(name, {
      snapshot: value,
      editor: model.editor,
      textEditable: model.textEditable,
      visualOptions: model.visualOptions,
      customProvider: model.customProvider,
      authMode: model.auth?.mode ?? "api-key",
      authKey: model.auth?.key ?? "",
      rawDiagnostics: [],
      saveFailure: null,
      readError: null,
      loading: false,
    });
  }
  reloadFile = async (name: string) => {
    if (!this.active || !this.snapshot.files[name]) return;
    const generation = (this.loads.get(name) ?? 0) + 1;
    this.loads.set(name, generation);
    const epoch = this.epoch;
    const current = () =>
      this.active && epoch === this.epoch && this.loads.get(name) === generation;
    this.clearTimer(`feedback:${name}`);
    this.patch(name, emptyFile());
    try {
      const value = await this.api.revealConfigFile({ ...this.target, file: name });
      if (!current()) return;
      this.accept(name, value);
      if (name !== "auth.json") {
        const visualAvailable = Boolean(value.visual_options && !value.visual_error);
        let mode = this.snapshot.mode;
        if (!visualAvailable) mode = "raw";
        if (visualAvailable && !this.visualInitialized && !this.target.current) {
          this.visualInitialized = true;
          mode = "visual";
        }
        this.commit({ visualAvailable, mode });
      }
      this.diagnose(name);
    } catch (cause) {
      if (!current()) return;
      const readError = messageOf(cause);
      this.patch(name, { loading: false, readError });
      this.onError(readError);
    }
  };
  reloadFiles = (names: readonly string[]) => {
    for (const name of names) void this.reloadFile(name);
  };
  retryReveals = () => {
    this.reloadFiles(this.names.filter((name) => this.snapshot.files[name].readError !== null));
  };
  private diagnose(name: string) {
    const data = this.file(name);
    const generation = (this.diagnoses.get(name) ?? 0) + 1;
    this.diagnoses.set(name, generation);
    this.clearTimer(`diagnose:${name}`);
    if (this.snapshot.mode !== "raw" || !data.snapshot || !data.textEditable) return;
    const epoch = this.epoch;
    const current = () =>
      this.active && this.epoch === epoch && this.diagnoses.get(name) === generation;
    this.later(`diagnose:${name}`, 250, () => {
      void this.api
        .diagnoseConfigFile(
          { ...this.target, file: name },
          encodeBase64(new TextEncoder().encode(data.editor)),
        )
        .then((value) => {
          if (current())
            this.patch(name, {
              rawDiagnostics: Array.isArray(value.diagnostics) ? value.diagnostics : [],
            });
        })
        .catch(() => {
          if (current()) this.patch(name, { rawDiagnostics: [] });
        });
    });
  }
  updateEditor = (name: string, editor: string) => {
    this.patch(name, { editor, saveFailure: null });
    this.diagnose(name);
  };
  setAuthKey = (name: string, authKey: string) => {
    this.patch(name, { authKey, saveFailure: null });
  };
  setAuthMode = (name: string, authMode: ConfigAuthData["mode"]) => {
    this.patch(name, { authMode, saveFailure: null });
  };
  updateVisualOption = (name: string, path: string, update: Partial<ConfigVisualOption>) => {
    const fields = this.snapshot.files[name].visualOptions;
    this.patch(name, {
      saveFailure: null,
      visualOptions: fields
        ? omitUnavailableOptions(
            fields.map((field) => (field.path === path ? { ...field, ...update } : field)),
          )
        : null,
    });
  };
  updateCustomProvider = (name: string, update: Partial<ConfigCustomProvider>) => {
    const provider = this.snapshot.files[name].customProvider;
    this.patch(name, {
      saveFailure: null,
      customProvider: provider ? { ...provider, ...update } : null,
    });
  };
  save = async (name: string): Promise<boolean> => {
    const data = this.file(name);
    const { snapshot, editorBytes, canSave, isAuth, customProvider, visualOptions } = data;
    const mode = this.snapshot.mode;
    if (!this.active || !snapshot || !editorBytes || !canSave) return false;
    if (
      name === "config.toml" &&
      mode === "visual" &&
      customProvider?.included &&
      this.snapshot.files["auth.json"] &&
      this.file("auth.json").dirty
    ) {
      this.onError("Save auth.json before saving a Custom provider configuration.");
      return false;
    }
    if (mode === "visual" && !isAuth) {
      const failure = visualSaveFailure(
        visualOptions,
        customProvider,
        requestProxyRoute(this.target.tenant, this.api.bootstrap?.listen),
      );
      if (failure) {
        this.patch(name, { saveFailure: failure });
        return false;
      }
    }
    if (
      mode === "visual" &&
      isAuth &&
      snapshot.auth?.extra_fields &&
      !window.confirm("Replace the extra native credential fields with an API-key object?")
    )
      return false;
    const epoch = this.epoch;
    const generation = this.loads.get(name);
    const current = () =>
      this.active && this.epoch === epoch && this.loads.get(name) === generation;
    this.clearTimer(`feedback:${name}`);
    this.patch(name, { saveFailure: null, feedback: "saving" });
    try {
      const value = await this.api.saveConfigFile(
        { ...this.target, file: name },
        configFileInput({ ...data, snapshot, editorBytes, mode }),
      );
      if (!current()) return false;
      this.accept(name, value);
      if (value.linked_file) void this.reloadFile(value.linked_file.file);
      this.patch(name, { feedback: "saved" });
      this.diagnose(name);
      this.onError(null);
      this.onSaved();
      this.later(`feedback:${name}`, 4000, () => {
        if (current()) this.patch(name, { feedback: "idle" });
      });
      return true;
    } catch (cause) {
      if (current())
        this.patch(name, {
          feedback: "idle",
          saveFailure: { message: messageOf(cause), paths: [] },
        });
      return false;
    }
  };
  saveInOrder = async (names: readonly string[]): Promise<boolean> => {
    for (const name of names) {
      if (!this.active) return false;
      if (this.snapshot.files[name] && this.file(name).dirty && !(await this.save(name)))
        return false;
    }
    return true;
  };
  requestAction = (
    run: () => void | Promise<void>,
    kind: ConfigPendingAction["kind"] = "leave",
  ) => {
    if (this.dirtyFiles.length) this.commit({ pendingAction: { run, kind } });
    else void run();
  };
  deferLeave = (run: () => void | Promise<void>) => {
    this.commit({ pendingAction: { run, kind: "leave" } });
  };
  cancelPending = () => {
    this.commit({ pendingAction: null });
  };
  savePending = async (names: readonly string[]) => {
    const action = this.snapshot.pendingAction;
    if (!action || !(await this.saveInOrder(names))) return;
    this.cancelPending();
    await action.run();
  };
  discardPending = async () => {
    const action = this.snapshot.pendingAction;
    if (!action) return;
    for (const name of this.dirtyFiles) {
      const snapshot = this.snapshot.files[name].snapshot;
      if (snapshot) {
        this.accept(name, snapshot);
        this.diagnose(name);
      }
    }
    this.onError(null);
    this.cancelPending();
    await action.run();
  };
  switchEditorMode = (mode: "visual" | "raw") => {
    if (mode === this.snapshot.mode) return;
    if (mode === "visual" && (!this.snapshot.visualAvailable || this.target.current)) {
      this.onError("Visual Editor is available only for a valid Named Config main file.");
      return;
    }
    this.requestAction(
      () => {
        this.commit({ mode });
        this.onError(null);
        for (const name of this.names) this.diagnose(name);
      },
      { switchTo: mode },
    );
  };
  showRawEditor = () => {
    this.switchEditorMode("raw");
  };
}

export type ConfigFileView = ReturnType<ConfigEditorSession["file"]>;
