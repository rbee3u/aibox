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
}

export interface LatestRequestLease {
  signal: AbortSignal;
  isCurrent(): boolean;
  release(): void;
}
