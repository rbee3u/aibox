import { describe, expect, it } from "vitest";
import {
  decodeHeader,
  mergeEventTimings,
  requestDetailUrl,
  requestUrl,
} from "@/features/requests/requestFormat";

describe("Request display formatting", () => {
  it("preserves binary header values as hex", () => {
    expect(decodeHeader({ name: "x-binary", value_base64: btoa("\xff") })).toBe("[hex] ff");
  });

  it("keeps corrupt header values from crashing the detail view", () => {
    expect(decodeHeader({ name: "x-invalid", value_base64: "%%%" })).toBe(
      "[invalid base64 header value]",
    );
  });

  it("derives list labels", () => {
    expect(
      requestUrl({ upstream_url: "https://api.example/v1?a=1", incoming_uri: "/raw" }),
    ).toEqual({
      host: "api.example",
      path: "/v1",
      label: "api.example/v1",
      title: "https://api.example/v1?a=1",
    });
    expect(requestUrl({ upstream_url: null, incoming_uri: "/raw" })).toEqual({
      host: "invalid target",
      path: "/raw",
      label: "invalid target /raw",
      title: "/raw",
    });
  });

  it("derives detail labels while preserving the full origin", () => {
    expect(
      requestDetailUrl({
        upstream_url: "https://api.example/v1?a=1",
        incoming_uri: "/raw",
      }),
    ).toEqual(["https://api.example", "/v1?a=1"]);
    expect(requestDetailUrl({ upstream_url: null, incoming_uri: "/raw" })).toEqual([
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
