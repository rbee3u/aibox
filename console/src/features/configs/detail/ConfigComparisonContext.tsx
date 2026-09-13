import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type {
  ConfigApi,
  ConfigComparison,
  ConfigComparisonInput,
  ConfigFileTarget,
} from "@/api/configs";
import { messageOf } from "@/shared/lib/errors";

export interface ConfigComparisonDraft {
  input: ConfigComparisonInput;
  dirty: boolean;
}
interface ComparisonContext {
  register: (file: string, draft: ConfigComparisonDraft | null) => void;
  result: ConfigComparison | null;
  pending: boolean;
  error: string | null;
  enabled: boolean;
  dirty: boolean;
  current: boolean;
}
const Context = createContext<ComparisonContext>({
  register: () => {},
  result: null,
  pending: false,
  error: null,
  enabled: false,
  dirty: false,
  current: false,
});
export function useConfigComparison() {
  return useContext(Context);
}

export function ConfigComparisonProvider({
  api,
  target,
  enabled,
  files,
  refresh,
  children,
}: {
  api: ConfigApi;
  target: Omit<ConfigFileTarget, "file">;
  enabled: boolean;
  files: readonly string[];
  refresh: unknown;
  children: ReactNode;
}) {
  const [registry, setRegistry] = useState<{
    entries: Record<string, ConfigComparisonDraft>;
    generation: number;
  }>({ entries: {}, generation: 0 });
  const drafts = registry.entries;
  const [response, setResponse] = useState<{
    key: string;
    refresh: unknown;
    result: ConfigComparison | null;
    error: string | null;
  } | null>(null);
  const register = useCallback((file: string, draft: ConfigComparisonDraft | null) => {
    setRegistry((previous) => {
      if (JSON.stringify(previous.entries[file] ?? null) === JSON.stringify(draft)) return previous;
      const next = { ...previous.entries };
      if (draft) next[file] = draft;
      else delete next[file];
      return { entries: next, generation: previous.generation + 1 };
    });
  }, []);
  const ready = files.some((file) => drafts[file]);
  const key = JSON.stringify({
    generation: registry.generation,
    target,
    inputs: files.flatMap((file) => (drafts[file] ? [drafts[file].input] : [])),
  });
  useEffect(() => {
    if (!enabled || !ready) return;
    let active = true;
    const timer = window.setTimeout(() => {
      const request = JSON.parse(key) as {
        target: Omit<ConfigFileTarget, "file">;
        inputs: ConfigComparisonInput[];
      };
      void api
        .compareConfigs(request.target, request.inputs)
        .then((result) => {
          if (active) setResponse({ key, refresh, result, error: null });
        })
        .catch((cause) => {
          if (active) setResponse({ key, refresh, result: null, error: messageOf(cause) });
        });
    }, 250);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [api, enabled, ready, key, refresh]);
  const matching =
    enabled && ready && response?.key === key && response.refresh === refresh ? response : null;
  const current = target.current;
  const value = useMemo(
    () => ({
      register,
      result: matching?.result ?? null,
      error: matching?.error ?? null,
      pending: enabled && !matching,
      enabled,
      dirty: Object.values(drafts).some((draft) => draft.dirty),
      current,
    }),
    [register, matching, enabled, drafts, current],
  );
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
