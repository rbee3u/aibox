import type { EventTimingIndex, RecordDetail, RecordList, RequestApi } from "./types";
import { HttpError, readHttpError } from "./httpError";

export { HttpError as ApiError } from "./httpError";

export function requestErrorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : "Requests API call failed";
}

export function requestWasCancelled(cause: unknown, signal: AbortSignal): boolean {
  return (
    signal.aborted ||
    (typeof cause === "object" && cause !== null && "name" in cause && cause.name === "AbortError")
  );
}

export function createRequestApi(fetchImpl: typeof fetch, csrfToken: string): RequestApi {
  const recordPath = (id: string) => `/_aibox/requests/api/records/${encodeURIComponent(id)}`;

  async function request(path: string, init: RequestInit = {}): Promise<Response> {
    const response = await fetchImpl.call(window, path, { ...init, cache: "no-store" });
    if (!response.ok) {
      throw new HttpError(await readHttpError(response), response.status);
    }
    return response;
  }

  async function requestJson<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await request(path, init);
    return (await response.json()) as T;
  }

  async function deleteRecordsAt(
    path: string,
    body: object,
    signal?: AbortSignal,
  ): Promise<number> {
    const payload = await requestJson<{ deleted: number }>(path, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Aibox-Csrf": csrfToken,
      },
      body: JSON.stringify(body),
      signal,
    });
    return payload.deleted;
  }

  return {
    listRecords(page = 1, signal) {
      const query = page === 1 ? "" : `?page=${page}`;
      return requestJson<RecordList>(`/_aibox/requests/api/records${query}`, { signal });
    },

    getRecord(id, signal) {
      return requestJson<RecordDetail>(recordPath(id), { signal });
    },

    async loadBody(id, kind, offset, signal) {
      const response = await request(`${recordPath(id)}/${kind}-body?offset=${offset}`, { signal });
      const bytes = new Uint8Array(await response.arrayBuffer());
      const header = response.headers.get("X-Aibox-Request-Next-Offset");
      const fallbackOffset = offset + bytes.length;
      const advertisedOffset = header === null ? null : Number(header);
      const nextOffset =
        advertisedOffset !== null &&
        Number.isSafeInteger(advertisedOffset) &&
        advertisedOffset === fallbackOffset
          ? advertisedOffset
          : fallbackOffset;
      return { bytes, nextOffset };
    },

    async loadDecodedBody(id, kind, signal) {
      const response = await request(`${recordPath(id)}/${kind}-body-decoded`, { signal });
      return new Uint8Array(await response.arrayBuffer());
    },

    loadEventTimings(id, afterSequence, signal) {
      return requestJson<EventTimingIndex>(
        `${recordPath(id)}/response-event-timings?after_sequence=${afterSequence}`,
        { signal },
      );
    },

    deleteRecords(ids, signal) {
      return deleteRecordsAt("/_aibox/requests/api/records/delete", { ids }, signal);
    },
  };
}
