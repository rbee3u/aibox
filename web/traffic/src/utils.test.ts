import { describe, expect, it } from "vitest";
import {
  bytes,
  compactDuration,
  concatChunks,
  decodeBytes,
  decodeHeader,
  duration,
  formatTimestamp,
  mergeEventTimings,
  recordDetailUrl,
  recordUrl,
} from "./utils";

describe("traffic display utilities", () => {
  it("formats timestamps in fixed UTC+08:00", () => {
    expect(formatTimestamp("2026-08-06T04:00:00Z")).toBe("2026-08-06 12:00:00");
    expect(formatTimestamp("2026-08-06T16:30:45Z")).toBe("2026-08-07 00:30:45");
    expect(formatTimestamp("2026-08-06T04:00:00")).toBe("—");
    expect(formatTimestamp("not-a-timestamp")).toBe("—");
  });

  it("formats compact list durations with whole-second precision", () => {
    expect(compactDuration(null)).toBe("—");
    expect(compactDuration(999)).toBe("999ms");
    expect(compactDuration(1000)).toBe("1s");
    expect(compactDuration(1499)).toBe("1s");
    expect(compactDuration(1500)).toBe("2s");
    expect(compactDuration(59999)).toBe("1m");
    expect(compactDuration(123000)).toBe("2m3s");
    expect(compactDuration(3603000)).toBe("1h3s");
    expect(compactDuration(3723000)).toBe("1h2m3s");
  });

  it("formats durations and byte counts", () => {
    expect(duration(null)).toBe("—");
    expect(duration(800)).toBe("800 ms");
    expect(duration(1250)).toBe("1.25 s");
    expect(bytes(12)).toBe("12 B");
    expect(bytes(2048)).toBe("2.0 KB");
  });

  it("preserves binary bodies and headers as hex", () => {
    const chunks = [new Uint8Array([0x61, 0x62]), new Uint8Array([0xff])];
    expect(concatChunks(chunks)).toEqual(new Uint8Array([0x61, 0x62, 0xff]));
    expect(decodeBytes(concatChunks(chunks), "body")).toBe("[non-UTF-8 body; hex view]\n61 62 ff");
    expect(decodeHeader({ name: "x-binary", value_base64: btoa("\xff") })).toBe("[hex] ff");
  });

  it("keeps corrupt header values from crashing the detail view", () => {
    expect(decodeHeader({ name: "x-invalid", value_base64: "%%%" })).toBe(
      "[invalid base64 header value]",
    );
  });

  it("derives list labels", () => {
    expect(recordUrl({ upstream_url: "https://api.example/v1?a=1", incoming_uri: "/raw" })).toEqual(
      {
        host: "api.example",
        path: "/v1",
        label: "api.example/v1",
        title: "https://api.example/v1?a=1",
      },
    );
    expect(recordUrl({ upstream_url: null, incoming_uri: "/raw" })).toEqual({
      host: "invalid target",
      path: "/raw",
      label: "invalid target /raw",
      title: "/raw",
    });
  });

  it("derives detail labels while preserving the full origin", () => {
    expect(
      recordDetailUrl({
        upstream_url: "https://api.example/v1?a=1",
        incoming_uri: "/raw",
      }),
    ).toEqual(["https://api.example", "/v1?a=1"]);
    expect(recordDetailUrl({ upstream_url: null, incoming_uri: "/raw" })).toEqual([
      "invalid target",
      "/raw",
    ]);
  });

  it("merges SSE timing snapshots by sequence while adopting the latest state", () => {
    expect(
      mergeEventTimings(
        {
          state: "partial",
          events: [
            { sequence: 3, completed_at_ns: "300" },
            { sequence: 1, completed_at_ns: "old" },
          ],
          next_sequence: 4,
          warning: "incomplete tail",
        },
        {
          state: "available",
          events: [
            { sequence: 2, completed_at_ns: "200" },
            { sequence: 1, completed_at_ns: "100" },
          ],
          next_sequence: 3,
          warning: null,
        },
      ),
    ).toEqual({
      state: "available",
      events: [
        { sequence: 1, completed_at_ns: "100" },
        { sequence: 2, completed_at_ns: "200" },
        { sequence: 3, completed_at_ns: "300" },
      ],
      next_sequence: 4,
      warning: null,
    });
  });
});
