import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RecordAssessment, RecordState, ResponseMetadata } from "../types";
import { RecordHeadlineStatus, RecordStatus } from "./RecordStatus";
import styles from "./RecordStatus.module.css";
import {
  assessmentPrimaryLabel,
  errorKindLabel,
  recordHeadlinePresentation,
  recordStatusPresentation,
} from "./statusPresentation";

const ok: RecordAssessment = { level: "ok", primary: null, issue_count: 0 };
const active: RecordAssessment = { level: "active", primary: null, issue_count: 0 };
const providerError: RecordAssessment = {
  level: "error",
  primary: {
    source: "provider",
    kind: "server_error",
    message: "Our servers are currently overloaded. Please try again later.",
  },
  issue_count: 2,
};
const disconnectWarning: RecordAssessment = {
  level: "warning",
  primary: {
    source: "traffic",
    kind: "client_disconnected",
    message: "The client disconnected before the response stream completed.",
  },
  issue_count: 1,
};

afterEach(() => vi.useRealTimers());

function presentation(
  status: number | null,
  assessment: RecordAssessment = ok,
  state: RecordState = "completed",
) {
  return recordStatusPresentation({ status, assessment, state });
}

const response: ResponseMetadata = {
  status: 200,
  source: "upstream",
  headers_at: "2026-08-06T04:00:00.100Z",
  http_version: "HTTP/2",
  reason_phrase: "OK",
  headers: [],
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
    expect(presentation(null, active, "active")).toEqual({
      label: "Waiting",
      tone: "active",
      issue: null,
      phase: null,
    });
    expect(presentation(200, active, "active")).toEqual({
      label: "200",
      tone: "success",
      issue: null,
      phase: "Streaming",
    });
  });

  it("keeps a missing HTTP response neutral while exposing its assessment", () => {
    expect(presentation(null, disconnectWarning)).toMatchObject({
      label: "No response",
      tone: "neutral",
      issue: { label: "Client disconnected", tone: "warning" },
    });
  });

  it("maps known and future finding kinds and formats HTTP findings", () => {
    expect(errorKindLabel("connect_timeout")).toBe("Connect timeout");
    expect(errorKindLabel("future_transport_error")).toBe("Future transport error");
    expect(
      assessmentPrimaryLabel({ source: "http", kind: "http_401", message: "Unauthorized" }),
    ).toBe("HTTP 401");
  });

  it("keeps HTTP 200 and a Provider Error separate", () => {
    expect(recordHeadlinePresentation(response, "completed", providerError)).toEqual({
      statusText: "HTTP/2 200 OK",
      tone: "success",
      tag: {
        label: "Server error",
        message: "Our servers are currently overloaded. Please try again later.",
        tone: "error",
        additionalIssues: 1,
      },
    });
  });
});

describe("RecordStatus", () => {
  it("shows Waiting and Streaming text in compact list status", () => {
    const { rerender } = render(
      <RecordStatus status={null} state="active" assessment={active} compact />,
    );
    expect(screen.getByText("Waiting")).toHaveClass(styles.active);

    rerender(<RecordStatus status={200} state="active" assessment={active} compact />);
    expect(screen.queryByText("HTTP/2")).not.toBeInTheDocument();
    expect(screen.getByText("200")).toHaveClass(styles.success);
    expect(screen.getByText("Streaming")).toBeInTheDocument();
  });

  it("renders one compact accessible error icon without replacing HTTP 200", () => {
    render(<RecordStatus status={200} state="completed" assessment={providerError} compact />);
    expect(screen.getByText("200")).toHaveClass(styles.success);
    const marker = screen.getByRole("img", {
      name: /Record error: Server error.*currently overloaded/,
    });
    expect(marker).toHaveClass(styles.error);
    expect(marker).not.toHaveAttribute("title");
  });

  it("shows fast custom tooltips for compact errors and warnings", () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <RecordStatus status={200} state="completed" assessment={providerError} compact />,
    );
    const errorMarker = screen.getByRole("img", { name: /Record error: Server error/ });

    fireEvent.pointerEnter(errorMarker);
    fireEvent.scroll(window);
    act(() => {
      vi.advanceTimersByTime(150);
    });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    fireEvent.pointerEnter(errorMarker);
    act(() => {
      vi.advanceTimersByTime(149);
    });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    const errorTooltip = screen.getByRole("tooltip");
    expect(errorMarker).toHaveAttribute("aria-describedby", errorTooltip.id);
    expect(errorTooltip).toHaveTextContent("Error · Server error");
    expect(errorTooltip).toHaveTextContent(providerError.primary!.message);

    fireEvent.pointerLeave(errorMarker);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    rerender(
      <RecordStatus status={null} state="completed" assessment={disconnectWarning} compact />,
    );
    const warningMarker = screen.getByRole("img", {
      name: /Record warning: Client disconnected/,
    });
    fireEvent.pointerEnter(warningMarker);
    act(() => {
      vi.advanceTimersByTime(150);
    });
    expect(within(screen.getByRole("tooltip")).getByText("Warning")).toHaveClass(
      styles.warningTone,
    );

    fireEvent.scroll(window);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("renders warning and error headline tags with fast tooltips", () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <RecordHeadlineStatus response={response} state="completed" assessment={providerError} />,
    );
    expect(screen.getByLabelText("HTTP/2 200 OK")).toHaveClass(styles.success);
    expect(screen.getByText("HTTP/2")).toHaveClass(styles.protocol);
    expect(screen.getByText("Server error").parentElement).toHaveClass(styles.errorTag);
    expect(screen.getByText("+1")).toBeInTheDocument();
    expect(screen.getByText("Server error").parentElement).not.toHaveAttribute("title");

    rerender(
      <RecordHeadlineStatus response={null} state="completed" assessment={disconnectWarning} />,
    );
    expect(screen.getByLabelText("No response")).toHaveClass(styles.neutral);
    const warningTag = screen.getByText("Client disconnected").parentElement!;
    expect(warningTag).toHaveClass(styles.warningTag);

    fireEvent.pointerEnter(warningTag);
    act(() => {
      vi.advanceTimersByTime(149);
    });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByRole("tooltip")).toHaveTextContent("Warning · Client disconnected");
    expect(screen.getByRole("tooltip")).toHaveTextContent(disconnectWarning.primary!.message);

    fireEvent.pointerLeave(warningTag);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("renders Waiting before response metadata arrives", () => {
    render(<RecordHeadlineStatus response={null} state="active" assessment={active} />);
    expect(screen.getByText("Waiting").parentElement).toHaveClass(styles.activeTag);
    expect(screen.queryByText("No response")).not.toBeInTheDocument();
  });
});
