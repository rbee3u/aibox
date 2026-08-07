import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { RecordState, ResponseMetadata, ResultMetadata } from "../types";
import { RecordHeadlineStatus, RecordStatus } from "./RecordStatus";
import styles from "./RecordStatus.module.css";
import {
  errorKindLabel,
  outcomeLabel,
  recordErrorPresentation,
  recordHeadlinePresentation,
  recordStatusPresentation,
} from "./statusPresentation";

function presentation(
  status: number | null,
  outcome = "completed",
  state: RecordState = "completed",
) {
  return recordStatusPresentation({ status, outcome, state });
}

const response: ResponseMetadata = {
  status: 200,
  source: "upstream",
  headers_at: "2026-08-06T04:00:00.100Z",
  http_version: "HTTP/2",
  reason_phrase: "OK",
  headers: [],
};

const failedResult: ResultMetadata = {
  ended_at: "2026-08-06T04:00:01Z",
  outcome: "upstream_error",
  total_ms: 1000,
  error: {
    kind: "upstream_response_failed",
    message: "upstream response stream failed: connection reset",
  },
};

describe("record status presentation", () => {
  it.each([
    [100, "neutral"],
    [200, "success"],
    [299, "success"],
    [302, "neutral"],
    [400, "error"],
    [524, "error"],
    [599, "error"],
    [99, "error"],
    [600, "error"],
  ] as const)("classifies HTTP status %i as %s", (status, tone) => {
    expect(presentation(status)).toMatchObject({ label: String(status), tone });
  });

  it("distinguishes waiting from streaming active records", () => {
    expect(presentation(null, "active", "active")).toEqual({
      label: "Waiting",
      tone: "active",
      anomaly: null,
      phase: null,
    });
    expect(presentation(200, "active", "active")).toEqual({
      label: "200",
      tone: "success",
      anomaly: null,
      phase: "Streaming",
    });
  });

  it("maps specific error kinds and keeps unknown values readable", () => {
    expect(errorKindLabel("connect_timeout")).toBe("Connect timeout");
    expect(errorKindLabel("future_transport_error")).toBe("Future transport error");
    expect(outcomeLabel("future_outcome")).toBe("Future outcome");
  });

  it("keeps HTTP status and the terminal lifecycle error separate", () => {
    expect(recordHeadlinePresentation(response, failedResult, "completed")).toEqual({
      statusText: "HTTP/2 200 OK",
      tone: "success",
      tag: "Upstream stream failed",
      tagTone: "error",
    });
    expect(recordErrorPresentation({ result: failedResult, state: "completed" })).toEqual({
      label: "Upstream stream failed",
      message: "upstream response stream failed: connection reset",
    });
  });

  it("synthesizes an interrupted error without a result", () => {
    expect(recordErrorPresentation({ result: null, state: "interrupted" })).toEqual({
      label: "Interrupted",
      message: "Traffic Proxy stopped before the Traffic Record was finalized.",
    });
  });
});

describe("RecordStatus", () => {
  it("shows Waiting and Streaming text in compact list status", () => {
    const { rerender } = render(
      <RecordStatus status={null} outcome="active" state="active" compact />,
    );
    expect(screen.getByText("Waiting")).toHaveClass(styles.active);

    rerender(<RecordStatus status={200} outcome="active" state="active" compact />);
    expect(screen.getByText("200")).toHaveClass(styles.success);
    expect(screen.getByText("Streaming")).toBeInTheDocument();
  });

  it("renders the complete response line and specific anomaly tag", () => {
    render(<RecordHeadlineStatus response={response} result={failedResult} state="completed" />);
    expect(screen.getByText("HTTP/2 200 OK")).toHaveClass(styles.success);
    expect(screen.getByText("Upstream stream failed")).toHaveClass(styles.errorTag);
  });

  it("renders Waiting before response metadata arrives", () => {
    render(<RecordHeadlineStatus response={null} result={null} state="active" />);
    expect(screen.getByText("Waiting")).toHaveClass(styles.activeTag);
    expect(screen.queryByText("No response")).not.toBeInTheDocument();
  });
});
