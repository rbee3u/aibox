import { describe, expect, it } from "vitest";
import {
  bytes,
  concatChunks,
  decodeBytes,
  decodeHeader,
  duration,
  queryParams,
  recordUrl,
} from "./utils";

describe("traffic display utilities", () => {
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

  it("derives list labels and repeated query parameters", () => {
    expect(recordUrl({ upstream_url: "https://api.example/v1?a=1", incoming_uri: "/raw" })).toEqual(
      ["api.example", "/v1?a=1"],
    );
    expect(recordUrl({ upstream_url: null, incoming_uri: "/raw" })).toEqual([
      "invalid target",
      "/raw",
    ]);
    expect(queryParams("https://api.example/v1?tag=a&tag=")).toEqual([
      ["tag", "a"],
      ["tag", ""],
    ]);
  });
});
