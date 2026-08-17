import { useEffect, useRef, useState } from "react";

const FEEDBACK_DURATION_MS = 1400;

export function useClipboardFeedback<T = true>() {
  const [copied, setCopied] = useState<T | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const request = useRef(0);

  useEffect(
    () => () => {
      request.current += 1;
      if (timer.current !== undefined) window.clearTimeout(timer.current);
    },
    [],
  );

  async function copy(text: string, value: T) {
    const requestId = ++request.current;
    if (timer.current !== undefined) {
      window.clearTimeout(timer.current);
      timer.current = undefined;
    }
    setCopied(null);
    try {
      await navigator.clipboard.writeText(text);
      if (request.current !== requestId) return;
      setCopied(value);
      timer.current = window.setTimeout(() => {
        timer.current = undefined;
        setCopied(null);
      }, FEEDBACK_DURATION_MS);
    } catch {
      if (request.current === requestId) setCopied(null);
    }
  }

  return [copied, copy] as const;
}
