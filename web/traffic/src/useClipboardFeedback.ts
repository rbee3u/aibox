import { useEffect, useRef, useState } from "react";

const FEEDBACK_DURATION_MS = 1400;

export function useClipboardFeedback<T = true>() {
  const [copied, setCopied] = useState<T | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(
    () => () => {
      if (timer.current !== undefined) window.clearTimeout(timer.current);
    },
    [],
  );

  async function copy(text: string, value: T) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(value);
      if (timer.current !== undefined) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => {
        timer.current = undefined;
        setCopied(null);
      }, FEEDBACK_DURATION_MS);
    } catch {
      setCopied(null);
    }
  }

  return [copied, copy] as const;
}
