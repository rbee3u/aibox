import type { EventTimingIndex, RecordDetail, RecordList, TrafficApi } from "./types";

export class ApiError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

export function requestErrorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : "Traffic management request failed";
}

export function requestWasCancelled(cause: unknown, signal: AbortSignal): boolean {
  return (
    signal.aborted ||
    (typeof cause === "object" && cause !== null && "name" in cause && cause.name === "AbortError")
  );
}

async function readError(response: Response): Promise<string> {
  try {
    const payload = (await response.json()) as { error?: unknown };
    if (typeof payload.error === "string" && payload.error) {
      return payload.error;
    }
  } catch {
    // The status text below is the useful fallback for a non-JSON error.
  }
  return `${response.status} ${response.statusText}`;
}

export function createTrafficApi(fetchImpl: typeof fetch = fetch): TrafficApi {
  const csrf = document.querySelector<HTMLMetaElement>('meta[name="aibox-csrf"]')?.content ?? "";
  const recordPath = (id: string) => `/_aibox/traffic/api/records/${encodeURIComponent(id)}`;

  async function request(path: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    const method = (init.method ?? "GET").toUpperCase();
    if (method !== "GET" && method !== "HEAD") {
      headers.set("X-Aibox-Traffic-CSRF", csrf);
    }
    const response = await fetchImpl(path, { ...init, headers, cache: "no-store" });
    if (!response.ok) {
      throw new ApiError(await readError(response), response.status);
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
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal,
    });
    return payload.deleted;
  }

  return {
    listRecords(page = 1, signal) {
      const query = page === 1 ? "" : `?page=${page}`;
      return requestJson<RecordList>(`/_aibox/traffic/api/records${query}`, { signal });
    },

    getRecord(id, signal) {
      return requestJson<RecordDetail>(recordPath(id), { signal });
    },

    async loadBody(id, kind, offset, signal) {
      const response = await request(`${recordPath(id)}/${kind}-body?offset=${offset}`, { signal });
      const bytes = new Uint8Array(await response.arrayBuffer());
      const header = response.headers.get("X-Aibox-Traffic-Next-Offset");
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
      return deleteRecordsAt("/_aibox/traffic/api/records/delete", { ids }, signal);
    },

    deleteAll(expectedDeletableCount, signal) {
      return deleteRecordsAt(
        "/_aibox/traffic/api/records/delete-all",
        { expected_deletable_count: expectedDeletableCount },
        signal,
      );
    },
  };
}
