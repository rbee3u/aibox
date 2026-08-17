import { describe, expect, it } from "vitest";
import {
  bytes,
  compactDuration,
  concatChunks,
  decodeHeader,
  duration,
  formatTimestamp,
  mergeEventTimings,
  recordDetailUrl,
  recordUrl,
} from "./utils";

describe("request display utilities", () => {
  it("formats timestamps in fixed UTC+08:00", () => {
    for (const [input, expected] of [
      ["2026-08-06T04:00:00Z", "2026-08-06 12:00:00"],
      ["2026-08-06T16:30:45Z", "2026-08-07 00:30:45"],
      ["2026-08-06T12:00:00+08:00", "2026-08-06 12:00:00"],
      ["2026-08-06T04:00:00", "—"],
      ["not-a-timestamp", "—"],
    ] as const) {
      expect(formatTimestamp(input), input).toBe(expected);
    }
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

  it("formats duration and byte-count unit boundaries", () => {
    for (const [input, expected] of [
      [null, "—"],
      [0, "0 ms"],
      [999, "999 ms"],
      [1000, "1.00 s"],
      [1250, "1.25 s"],
    ] as const) {
      expect(duration(input), String(input)).toBe(expected);
    }
    for (const [input, expected] of [
      [null, "—"],
      [0, "0 B"],
      [1023, "1023 B"],
      [1024, "1.0 KB"],
      [1048576, "1.0 MB"],
    ] as const) {
      expect(bytes(input), String(input)).toBe(expected);
    }
  });

  it("concatenates Body chunks and preserves binary headers as hex", () => {
    const chunks = [new Uint8Array([0x61, 0x62]), new Uint8Array([0xff])];
    expect(concatChunks(chunks)).toEqual(new Uint8Array([0x61, 0x62, 0xff]));
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
