import { useCallback, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import { readPreference, storePreference } from "./preferences";

export const DEFAULT_LIST_WIDTH = 480;
export const MIN_LIST_WIDTH = 360;
export const MAX_LIST_WIDTH = 640;
export const LIST_WIDTH_STEP = 16;

const STORAGE_KEY = "aibox-traffic-list-width";

interface SplitDrag {
  pointerId: number;
  startX: number;
  startWidth: number;
  currentWidth: number;
}

export function useResizableListWidth() {
  const [listWidth, setListWidth] = useState(readListWidth);
  const [resizing, setResizing] = useState(false);
  const splitDrag = useRef<SplitDrag | null>(null);

  const updateListWidth = useCallback((value: number, persist = false) => {
    const next = clampListWidth(value);
    setListWidth(next);
    if (persist) storePreference(STORAGE_KEY, String(next));
  }, []);

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    splitDrag.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: listWidth,
      currentWidth: listWidth,
    };
    setResizing(true);
    if (typeof event.currentTarget.setPointerCapture === "function") {
      event.currentTarget.setPointerCapture(event.pointerId);
    }
    event.preventDefault();
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const drag = splitDrag.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    drag.currentWidth = clampListWidth(drag.startWidth + event.clientX - drag.startX);
    updateListWidth(drag.currentWidth);
  }

  function onPointerUp(event: PointerEvent<HTMLDivElement>) {
    const drag = splitDrag.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    updateListWidth(drag.currentWidth, true);
    splitDrag.current = null;
    setResizing(false);
    if (
      typeof event.currentTarget.hasPointerCapture === "function" &&
      event.currentTarget.hasPointerCapture(event.pointerId)
    ) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    let next: number | null = null;
    if (event.key === "ArrowLeft") next = listWidth - LIST_WIDTH_STEP;
    if (event.key === "ArrowRight") next = listWidth + LIST_WIDTH_STEP;
    if (event.key === "Home") next = MIN_LIST_WIDTH;
    if (event.key === "End") next = MAX_LIST_WIDTH;
    if (next === null) return;
    event.preventDefault();
    updateListWidth(next, true);
  }

  const reset = useCallback(() => updateListWidth(DEFAULT_LIST_WIDTH, true), [updateListWidth]);

  return {
    listWidth,
    resizing,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onKeyDown,
    reset,
  };
}

function readListWidth(): number {
  const stored = readPreference(STORAGE_KEY);
  if (stored === null) return DEFAULT_LIST_WIDTH;
  const value = Number(stored);
  return Number.isFinite(value) ? clampListWidth(value) : DEFAULT_LIST_WIDTH;
}

function clampListWidth(value: number): number {
  return Math.min(MAX_LIST_WIDTH, Math.max(MIN_LIST_WIDTH, Math.round(value)));
}
