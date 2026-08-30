import { describe, expect, it } from "vitest";
import {
  activeDetail,
  completedDetail,
  withIncompleteRequestBody,
} from "@/features/requests/testFixtures";
import type { HeaderValue, RequestDetail, ResponseModeValue } from "@/api/requests";
import {
  bodyComplete,
  bodyMediaType,
  contentCoding,
  decodeUtf8,
  eventAbsoluteTime,
  eventRelativeTime,
  isJsonMediaType,
  isSseResponse,
  jsonStringPreview,
  LARGE_PRETTY_BYTES,
  LONG_STRING_CHARACTERS,
  parseJson,
  parseSse,
  sseEventTypes,
  shouldDeferPretty,
  stringifyJson,
} from "@/features/requests/detail/bodyPresentation";

function header(name: string, value: string): HeaderValue {
  return { name, value_base64: btoa(value) };
}

function detailWithResponse(mediaType: string, observed: ResponseModeValue): RequestDetail {
  return {
    ...completedDetail,
    response: {
      ...completedDetail.response,
      headers: [header("content-type", mediaType)],
    },
    summary: {
      ...completedDetail.summary,
      protocol: {
        ...completedDetail.summary.protocol,
        response_mode: { requested: "normal", observed },
      },
    },
  };
}

describe("Body presentation", () => {
  it.each([
    ["completed request", completedDetail, "request", true],
    ["completed response", completedDetail, "response", true],
    ["complete active request", activeDetail, "request", true],
    ["incomplete active request", withIncompleteRequestBody(activeDetail), "request", false],
    ["incomplete active response", activeDetail, "response", false],
  ] as const)("reports Body completeness for %s", (_name, detail, kind, expected) => {
    expect(bodyComplete(detail, kind)).toBe(expected);
  });

  it.each([
    ["SSE media type", detailWithResponse("text/event-stream", "normal"), true],
    ["observed stream mode", detailWithResponse("application/json", "stream"), true],
    ["normal response mode", detailWithResponse("application/json", "normal"), false],
    ["missing response", activeDetail, false],
  ] as const)("recognizes SSE for %s", (_name, detail, expected) => {
    expect(isSseResponse(detail)).toBe(expected);
  });

  it("classifies Content-Encoding and JSON media types case-insensitively", () => {
    expect(contentCoding([])).toEqual({ kind: "identity" });
    expect(contentCoding([header("Content-Encoding", " IdEnTiTy ")])).toEqual({
      kind: "identity",
    });
    expect(contentCoding([header("CONTENT-ENCODING", " ZsTd ")])).toEqual({ kind: "zstd" });
    expect(contentCoding([header("content-encoding", "gzip, zstd")])).toEqual({
      kind: "unsupported",
      message: "Unsupported Content-Encoding: gzip, zstd",
    });
    expect(contentCoding([{ name: "content-encoding", value_base64: btoa("\xff") }])).toEqual({
      kind: "unsupported",
      message: "Content-Encoding header is not valid UTF-8",
    });
    expect(bodyMediaType([header("Content-Type", "Application/Problem+JSON; charset=utf-8")])).toBe(
      "application/problem+json",
    );
    expect(isJsonMediaType("application/problem+json")).toBe(true);
    expect(isJsonMediaType("text/json-seq")).toBe(false);
  });

  it("preserves the original spelling of large and exponential JSON numbers", () => {
    const parsed = parseJson('{"big":900719925474099312345,"tiny":1.2300e-400}');
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    expect(stringifyJson(parsed.value)).toBe('{"big":900719925474099312345,"tiny":1.2300e-400}');
    expect(stringifyJson(parsed.value, true)).toContain("900719925474099312345");
  });

  it("rejects duplicate keys and invalid JSON instead of changing their evidence", () => {
    const duplicate = parseJson('{"a":1,"a":2}');
    expect(duplicate.ok).toBe(false);
    if (!duplicate.ok) expect(duplicate.message).toContain("Duplicate object key");
    expect(parseJson("{nope")).toMatchObject({ ok: false });
  });

  it("strictly distinguishes UTF-8 Source from original hex", () => {
    expect(decodeUtf8(new TextEncoder().encode("你好"))).toEqual({ ok: true, text: "你好" });
    expect(decodeUtf8(new Uint8Array([0xff, 0x00]))).toEqual({
      ok: false,
      hex: "ff 00",
    });
  });

  it("keeps an incomplete UTF-8 tail readable while an active body is still streaming", () => {
    expect(decodeUtf8(new Uint8Array([0xe4, 0xbd]), false)).toEqual({ ok: true, text: "" });
    expect(decodeUtf8(new Uint8Array([0xe4, 0xbd]), true)).toMatchObject({ ok: false });
    expect(decodeUtf8(new Uint8Array([0xff, 0xe4]), false)).toMatchObject({ ok: false });
  });

  it("applies the Pretty and string guards only above their exact boundaries", () => {
    expect(shouldDeferPretty(LARGE_PRETTY_BYTES)).toBe(false);
    expect(shouldDeferPretty(LARGE_PRETTY_BYTES + 1)).toBe(true);
    expect(jsonStringPreview("界".repeat(LONG_STRING_CHARACTERS))).toBeNull();
    expect(jsonStringPreview("😀".repeat(LONG_STRING_CHARACTERS + 1))).toBe(
      "😀".repeat(LONG_STRING_CHARACTERS),
    );
  });

  it("parses complete SSE Events with BOM, all line endings, multiline data, and comments", () => {
    const parsed = parseSse(
      '\uFEFF: keepalive\rdata: {"type":"answer.delta",\r\ndata: "text":"x"}\nevent: transport\r\n\r\n' +
        "data: [DONE]\n\n" +
        "event: ignored\ndata: tail",
    );
    expect(parsed.events).toHaveLength(2);
    expect(parsed.events[0]).toMatchObject({
      sequence: 0,
      eventType: "transport",
      explicitEventType: "transport",
      data: '{"type":"answer.delta",\n"text":"x"}',
    });
    expect(sseEventTypes(parsed.events[0])).toEqual({
      primary: "answer.delta",
      secondary: "transport",
    });
    expect(parsed.events[1]).toMatchObject({ data: "[DONE]", eventType: "message" });
    expect(parsed.hasPartialTail).toBe(true);
  });

  it("uses the payload object as the SSE Event type fallback", () => {
    const [event] = parseSse('data: {"object":"chat.completion.chunk","choices":[]}\n\n').events;

    expect(sseEventTypes(event)).toEqual({
      primary: "chat.completion.chunk",
      secondary: null,
    });
  });

  it("ignores blocks without data and does not dispatch a newline-only partial event", () => {
    expect(parseSse(": ping\n\nevent: nope\n\n").events).toEqual([]);
    expect(parseSse("data: value\n")).toEqual({ events: [], hasPartialTail: true });
  });

  it("formats SSE completion offsets with milliseconds and absolute time", () => {
    expect(eventRelativeTime("1250500000")).toBe("+1.251 s");
    expect(eventRelativeTime("12500000")).toBe("+12.5 ms");
    expect(eventAbsoluteTime(completedDetail.request.started_at, "1250500000")).toBe(
      "2026-08-06 12:00:01.251",
    );
    expect(eventRelativeTime("invalid")).toBe("Time unavailable");
    expect(
      eventAbsoluteTime(completedDetail.request.started_at, "1000000000000000000000000000000"),
    ).toBe("—");
  });
});
