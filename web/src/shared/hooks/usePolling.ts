import { useEffect, useRef } from "react";

interface PollingOptions {
  /** Polling stops and any in-flight work is cancelled while this is false. */
  enabled: boolean;
  intervalMs: number;
  /** Runs immediately on enable, then once per interval. `first` marks the initial run. */
  run: (first: boolean) => Promise<void>;
  /** Cancels work in flight when the loop stops or restarts. */
  onCancel?: () => void;
}

/**
 * Runs `run` immediately and then on a fixed interval measured from each
 * completion, so a slow response never overlaps the next attempt.
 */
export function usePolling({ enabled, intervalMs, run, onCancel }: PollingOptions) {
  const firstRun = useRef(true);
  const runRef = useRef(run);
  const cancelRef = useRef(onCancel);

  useEffect(() => {
    runRef.current = run;
    cancelRef.current = onCancel;
  }, [onCancel, run]);

  useEffect(() => {
    if (!enabled) {
      cancelRef.current?.();
      return;
    }
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      await runRef.current(firstRun.current);
      firstRun.current = false;
      if (!disposed) timer = window.setTimeout(() => void poll(), intervalMs);
    };
    void poll();
    return () => {
      disposed = true;
      cancelRef.current?.();
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [enabled, intervalMs]);
}
