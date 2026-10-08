import { useCallback, useEffect, useRef, useState } from "react";

import type { ComponentLatestSnapshot, TenantApi } from "@/api/tenants";
import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

type ComponentLatestApi = Pick<TenantApi, "latestComponents" | "checkLatestComponents">;

/** Owns the global Latest Release snapshot independently of Component state. */
export function useComponentLatest(
  api: ComponentLatestApi,
  onError: (message: string | null) => void,
) {
  const [snapshot, setSnapshot] = useState<ComponentLatestSnapshot | null>(null);
  const [checking, setChecking] = useState(false);
  const latestRequest = useRef(new LatestRequest());

  const check = useCallback(async (): Promise<ComponentLatestSnapshot | null> => {
    if (checking) return null;
    setChecking(true);
    try {
      const next = await api.checkLatestComponents();
      setSnapshot(next);
      return next;
    } catch (cause) {
      onError(messageOf(cause));
      return null;
    } finally {
      setChecking(false);
    }
  }, [api, checking, onError]);

  useEffect(() => {
    const owner = latestRequest.current;
    void owner.run((signal) => api.latestComponents(signal), {
      loaded: setSnapshot,
      failed: () => {
        // A missing observation is an expected first-run state.
      },
    });
    return () => owner.cancel();
  }, [api]);

  return { check, checking, snapshot };
}
