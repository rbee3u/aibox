import { describe, expect, it } from "vitest";
import {
  capitalize,
  compactDuration,
  concatChunks,
  duration,
  formatByteSize,
  formatTimestamp,
  formatTimestampWithMilliseconds,
  hex,
} from "@/shared/lib/format";

describe("shared display formatting", () => {
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

  it("adds millisecond precision only on request", () => {
    expect(formatTimestampWithMilliseconds("2026-08-06T04:00:00.250Z")).toBe(
      "2026-08-06 12:00:00.250",
    );
    expect(formatTimestamp("2026-08-06T04:00:00.250Z")).toBe("2026-08-06 12:00:00");
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
    expect(compactDuration(9020920)).toBe("2h30m21s");
  });

  it("formats duration unit boundaries", () => {
    for (const [input, expected] of [
      [null, "—"],
      [0, "0 ms"],
      [999, "999 ms"],
      [1000, "1.00 s"],
      [1250, "1.25 s"],
    ] as const) {
      expect(duration(input), String(input)).toBe(expected);
    }
  });

  it("formats decimal-prefix byte counts", () => {
    for (const [input, expected] of [
      [null, "—"],
      [0, "0 B"],
      [1023, "1023 B"],
      [1024, "1.0 KB"],
      [1048576, "1.0 MB"],
    ] as const) {
      expect(formatByteSize(input), String(input)).toBe(expected);
    }
  });

  it("capitalizes a leading character and leaves an empty value alone", () => {
    for (const [input, expected] of [
      ["", ""],
      ["built", "Built"],
      ["Built", "Built"],
      ["a", "A"],
      ["danger-full-access", "Danger-full-access"],
    ] as const) {
      expect(capitalize(input), JSON.stringify(input)).toBe(expected);
    }
  });

  it("concatenates binary chunks and renders bytes as spaced hex", () => {
    const chunks = [new Uint8Array([0x61, 0x62]), new Uint8Array([0xff])];
    expect(concatChunks(chunks)).toEqual(new Uint8Array([0x61, 0x62, 0xff]));
    expect(hex(new Uint8Array([0x00, 0x0f, 0xff]))).toBe("00 0f ff");
  });
});
