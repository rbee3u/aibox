import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type {
  ConfigApi,
  ConfigComparison,
  ConfigComparisonInput,
  ConfigFileTarget,
} from "@/api/configs";
import { messageOf } from "@/shared/lib/errors";

interface ComparisonContext {
  result: ConfigComparison | null;
  pending: boolean;
  error: string | null;
  enabled: boolean;
  dirty: boolean;
  current: boolean;
}
const Context = createContext<ComparisonContext>({
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
  inputs,
  revision,
  dirty,
  refresh,
  children,
}: {
  api: ConfigApi;
  target: Omit<ConfigFileTarget, "file">;
  enabled: boolean;
  inputs: ConfigComparisonInput[];
  revision: number;
  dirty: boolean;
  refresh: unknown;
  children: ReactNode;
}) {
  const [response, setResponse] = useState<{
    key: string;
    refresh: unknown;
    result: ConfigComparison | null;
    error: string | null;
  } | null>(null);
  const ready = inputs.length > 0;
  const key = JSON.stringify({ target, inputs, revision });
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
      result: matching?.result ?? null,
      error: matching?.error ?? null,
      pending: enabled && !matching,
      enabled,
      dirty,
      current,
    }),
    [matching, enabled, dirty, current],
  );
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
