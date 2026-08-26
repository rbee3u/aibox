import type { Bootstrap } from "@/api/core";
import { HttpError, readHttpError } from "@/api/httpError";

export { HttpError as ApiError } from "@/api/httpError";

export class ControlApi {
  readonly bootstrap: Bootstrap;
  private readonly fetchImpl: typeof fetch;

  constructor(bootstrap: Bootstrap, fetchImpl: typeof fetch = fetch) {
    this.bootstrap = bootstrap;
    this.fetchImpl = fetchImpl;
  }

  static async connect(fetchImpl: typeof fetch = fetch): Promise<ControlApi> {
    const response = await fetchImpl.call(window, "/_aibox/api/bootstrap", { cache: "no-store" });
    if (!response.ok) throw new HttpError(await readHttpError(response), response.status);
    return new ControlApi((await response.json()) as Bootstrap, fetchImpl);
  }

  async get<T>(path: string, signal?: AbortSignal): Promise<T> {
    const response = await this.getResponse(path, signal);
    return (await response.json()) as T;
  }

  async getResponse(path: string, signal?: AbortSignal): Promise<Response> {
    const response = await this.fetchImpl.call(window, path, { cache: "no-store", signal });
    if (!response.ok) throw new HttpError(await readHttpError(response), response.status);
    return response;
  }

  async post<T>(path: string, body: object = {}, signal?: AbortSignal): Promise<T> {
    const response = await this.fetchImpl.call(window, path, {
      method: "POST",
      cache: "no-store",
      headers: {
        "Content-Type": "application/json",
        "X-Aibox-Csrf": this.bootstrap.csrf_token,
      },
      body: JSON.stringify(body),
      signal,
    });
    if (!response.ok) throw new HttpError(await readHttpError(response), response.status);
    return (await response.json()) as T;
  }

  /**
   * Reads a newline-delimited JSON response and hands each complete record to
   * `onRecord`. Records may straddle body chunks, so the trailing partial line
   * is retained until its terminator arrives.
   */
  async streamNdjson<T>(
    path: string,
    onRecord: (record: T) => void,
    signal?: AbortSignal,
  ): Promise<void> {
    const response = await this.fetchImpl.call(window, path, { cache: "no-store", signal });
    if (!response.ok || !response.body) {
      throw new HttpError(await readHttpError(response), response.status);
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let pending = "";
    while (true) {
      const chunk = await reader.read();
      pending += decoder.decode(chunk.value, { stream: !chunk.done });
      const lines = pending.split("\n");
      pending = lines.pop() ?? "";
      for (const line of lines) {
        if (!line) continue;
        onRecord(JSON.parse(line) as T);
      }
      if (chunk.done) break;
    }
  }
}
