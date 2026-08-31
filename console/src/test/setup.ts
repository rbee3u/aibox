import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

// CodeMirror probes canvas text metrics, while jsdom deliberately does not
// implement a rendering context. Returning null exercises CodeMirror's native
// fallback without adding a platform-specific canvas package or noisy errors.
Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  configurable: true,
  value: () => null,
});

if (!globalThis.ResizeObserver) {
  class TestResizeObserver implements ResizeObserver {
    readonly #callback: ResizeObserverCallback;

    constructor(callback: ResizeObserverCallback) {
      this.#callback = callback;
    }

    observe(target: Element) {
      const bounds = target.getBoundingClientRect();
      const width =
        bounds.width || (target instanceof HTMLElement ? target.clientWidth : 0) || 1024;
      this.#callback(
        [
          {
            target,
            contentRect: {
              x: bounds.x,
              y: bounds.y,
              top: bounds.top,
              right: bounds.left + width,
              bottom: bounds.bottom,
              left: bounds.left,
              width,
              height: bounds.height,
              toJSON: () => ({}),
            },
          } as ResizeObserverEntry,
        ],
        this,
      );
    }

    unobserve() {}

    disconnect() {}
  }

  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    writable: true,
    value: TestResizeObserver,
  });
}

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute("style");
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-resolved-theme");
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});
