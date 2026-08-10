import type { BodyKind, EventTimingIndex, RecordDetail, RecordList, TrafficApi } from "./types";

export class ApiError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
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

  return {
    async listRecords(page = 1, signal) {
      const query = page === 1 ? "" : `?page=${page}`;
      const response = await request(`/_aibox/traffic/api/records${query}`, { signal });
      return (await response.json()) as RecordList;
    },

    async getRecord(id, signal) {
      const response = await request(`/_aibox/traffic/api/records/${encodeURIComponent(id)}`, {
        signal,
      });
      return (await response.json()) as RecordDetail;
    },

    async loadBody(id, kind, offset, signal) {
      const response = await request(
        `/_aibox/traffic/api/records/${encodeURIComponent(id)}/${kind}-body?offset=${offset}`,
        { signal },
      );
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
      const response = await request(
        `/_aibox/traffic/api/records/${encodeURIComponent(id)}/${decodedBodyPath(kind)}`,
        { signal },
      );
      return new Uint8Array(await response.arrayBuffer());
    },

    async loadEventTimings(id, afterSequence, signal) {
      const response = await request(
        `/_aibox/traffic/api/records/${encodeURIComponent(id)}/response-event-timings?after_sequence=${afterSequence}`,
        { signal },
      );
      return (await response.json()) as EventTimingIndex;
    },

    async deleteRecords(ids, signal) {
      const response = await request("/_aibox/traffic/api/records/delete", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ids }),
        signal,
      });
      const payload = (await response.json()) as { deleted: number };
      return payload.deleted;
    },

    async deleteAll(expectedDeletableCount, signal) {
      const response = await request("/_aibox/traffic/api/records/delete-all", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ expected_deletable_count: expectedDeletableCount }),
        signal,
      });
      const payload = (await response.json()) as { deleted: number };
      return payload.deleted;
    },
  };
}

function decodedBodyPath(kind: BodyKind): string {
  return kind === "request" ? "request-body-decoded" : "response-body-decoded";
}
