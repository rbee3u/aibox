/** Owns one replaceable AbortController and identifies the current request. */
export class LatestRequest {
  private current: AbortController | null = null;

  begin(): LatestRequestLease {
    this.current?.abort();
    const controller = new AbortController();
    this.current = controller;
    return {
      signal: controller.signal,
      isCurrent: () => this.current === controller,
      release: () => {
        if (this.current === controller) this.current = null;
      },
    };
  }

  cancel(): void {
    this.current?.abort();
    this.current = null;
  }

  /** Commit a single response only while its lease still owns this resource. */
  async run<T>(
    load: (signal: AbortSignal) => Promise<T>,
    callbacks: LatestRequestCallbacks<T>,
  ): Promise<T | null> {
    const request = this.begin();
    try {
      const value = await load(request.signal);
      if (request.signal.aborted || !request.isCurrent()) return null;
      callbacks.loaded(value);
      return value;
    } catch (cause) {
      if (!request.signal.aborted && request.isCurrent()) callbacks.failed(cause);
      return null;
    } finally {
      if (request.isCurrent()) {
        request.release();
        callbacks.settled?.();
      }
    }
  }
}

export interface LatestRequestCallbacks<T> {
  loaded(value: T): void;
  failed(cause: unknown): void;
  settled?(): void;
}

export interface LatestRequestLease {
  signal: AbortSignal;
  isCurrent(): boolean;
  release(): void;
}
