import { useEffect, useRef, useState } from "react";

import { messageOf } from "@/shared/lib/errors";
import { LatestRequest } from "@/shared/lib/latestRequest";

/** Own one latest-request-wins resource snapshot without knowing its adapter. */
export function useAsyncResource<T>(load: (signal: AbortSignal) => Promise<T>, initial: T) {
  const [data, setData] = useState<T>(initial);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  const requestOwner = useRef(new LatestRequest());

  useEffect(() => {
    const owner = requestOwner.current;
    const request = owner.begin();
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setLoading(true);
    void load(request.signal)
      .then((value) => {
        if (request.signal.aborted || !request.isCurrent()) return;
        setData(value);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (!request.signal.aborted && request.isCurrent()) setError(messageOf(cause));
      })
      .finally(() => {
        if (request.isCurrent()) {
          request.release();
          setLoading(false);
        }
      });
    return () => owner.cancel();
  }, [generation, load]);

  return {
    data,
    loading,
    error,
    retry: () => setGeneration((value) => value + 1),
  };
}
