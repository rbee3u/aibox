import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RequestAssessment, RequestState, ResponseMetadata } from "@/api/requests";
import { RecordHeadlineStatus, RequestStatus } from "@/features/requests/RequestStatus";
import {
  assessmentPrimaryLabel,
  errorKindLabel,
  requestHeadlinePresentation,
  requestStatusPresentation,
} from "@/features/requests/statusPresentation";

const ok: RequestAssessment = { level: "ok", primary: null, issue_count: 0 };
const active: RequestAssessment = { level: "active", primary: null, issue_count: 0 };
const providerError: RequestAssessment = {
  level: "error",
  primary: {
    source: "provider",
    kind: "server_error",
    message: "Our servers are currently overloaded. Please try again later.",
  },
  issue_count: 2,
};
const disconnectWarning: RequestAssessment = {
  level: "warning",
  primary: {
    source: "request",
    kind: "client_disconnected",
    message: "The client disconnected before the response stream completed.",
  },
  issue_count: 1,
};

function presentation(
  status: number | null,
  assessment: RequestAssessment = ok,
  state: RequestState = "completed",
) {
  return requestStatusPresentation({ status, assessment, state });
}

const response: ResponseMetadata = {
  status: 200,
  source: "upstream",
  headers_at: "2026-08-06T04:00:00.100Z",
  http_version: "HTTP/2",
  reason_phrase: "OK",
  headers: [],
};

function showTooltip(target: HTMLElement) {
  fireEvent.pointerEnter(target);
  act(() => {
    vi.runOnlyPendingTimers();
  });
  return screen.getByRole("tooltip");
}

describe("request status presentation", () => {
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

  it("distinguishes waiting from streaming active requests", () => {
    expect(presentation(null, active, "active")).toEqual({
      label: "Waiting",
      tone: "active",
      issue: null,
      marker: null,
      phase: null,
    });
    expect(presentation(200, active, "active")).toEqual({
      label: "200",
      tone: "success",
      issue: null,
      marker: null,
      phase: "Streaming",
    });
  });

  it("names the finding when a completed Request never got an HTTP status", () => {
    expect(presentation(null, disconnectWarning)).toMatchObject({
      label: "Disconnected",
      tone: "warning",
      issue: { label: "Disconnected", tone: "warning" },
      marker: null,
    });
    expect(presentation(null, providerError)).toMatchObject({
      label: "Server error",
      tone: "error",
      marker: null,
    });
    expect(presentation(null)).toMatchObject({
      label: "No response",
      tone: "neutral",
      issue: null,
      marker: null,
    });
  });

  it("hangs a level marker on an HTTP status the assessment adds to", () => {
    expect(presentation(200, providerError)).toMatchObject({
      label: "200",
      tone: "error",
      issue: { label: "Server error" },
      marker: "error",
    });
    expect(presentation(200, disconnectWarning)).toMatchObject({
      label: "200",
      tone: "success",
      marker: "warning",
    });
    expect(presentation(400, disconnectWarning)).toMatchObject({
      label: "400",
      tone: "error",
      marker: "warning",
    });
  });

  it("carries no marker or hover reason when the assessment restates the HTTP status", () => {
    const http401: RequestAssessment = {
      level: "error",
      primary: { source: "http", kind: "http_401", message: "Upstream returned HTTP 401" },
      issue_count: 1,
    };
    expect(presentation(401, http401)).toEqual({
      label: "401",
      tone: "error",
      issue: null,
      marker: null,
      phase: null,
    });
    expect(presentation(200, http401)).toMatchObject({
      label: "200",
      tone: "error",
      issue: { label: "HTTP 401" },
      marker: "error",
    });
    expect(presentation(200, providerError, "active").issue).toBeNull();
  });

  it("maps known and future finding kinds and formats HTTP findings", () => {
    expect(errorKindLabel("connect_timeout")).toBe("Connect timeout");
    expect(errorKindLabel("future_transport_error")).toBe("Future transport error");
    expect(
      assessmentPrimaryLabel({ source: "http", kind: "http_401", message: "Unauthorized" }),
    ).toBe("HTTP 401");
    expect(errorKindLabel("upstream_request_failed")).toBe("Upstream failed");
  });

  it("keeps HTTP 200 and a Provider Error separate", () => {
    expect(requestHeadlinePresentation(response, "completed", providerError)).toEqual({
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

  it("does not hang a headline tag that only restates the HTTP status", () => {
    const http401: RequestAssessment = {
      level: "error",
      primary: {
        source: "http",
        kind: "http_401",
        message: "Upstream returned HTTP 401",
      },
      issue_count: 1,
    };
    expect(
      requestHeadlinePresentation(
        { ...response, status: 401, reason_phrase: "Unauthorized" },
        "completed",
        http401,
      ),
    ).toEqual({
      statusText: "HTTP/2 401 Unauthorized",
      tone: "error",
      tag: null,
    });
    expect(requestHeadlinePresentation(response, "completed", http401).tag).toMatchObject({
      label: "HTTP 401",
      tone: "error",
    });
  });
});

describe("RequestStatus", () => {
  it("shows Waiting and Streaming text in list status", () => {
    const { rerender } = render(<RequestStatus status={null} state="active" assessment={active} />);
    expect(screen.getByText("Waiting")).toBeInTheDocument();

    rerender(<RequestStatus status={200} state="active" assessment={active} />);
    expect(screen.queryByText("HTTP/2")).not.toBeInTheDocument();
    expect(screen.getByText("200")).toBeInTheDocument();
    expect(screen.getByText("Streaming")).toBeInTheDocument();
  });

  it("explains a finding from the whole status cell and keeps the model line free", () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <RequestStatus status={200} state="completed" assessment={providerError} />,
    );
    expect(screen.getByText("200")).toBeInTheDocument();
    expect(screen.queryByText("Server error")).not.toBeInTheDocument();
    const errorCell = screen.getByRole("img", {
      name: /Request error: Server error.*currently overloaded/,
    });
    expect(errorCell).not.toHaveAttribute("title");
    expect(errorCell.querySelector("[data-status-tone]")).toHaveAttribute(
      "data-status-tone",
      "error",
    );

    fireEvent.pointerEnter(errorCell);
    fireEvent.scroll(window);
    act(() => {
      vi.runOnlyPendingTimers();
    });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    const errorTooltip = showTooltip(errorCell);
    expect(errorCell).toHaveAttribute("aria-describedby", errorTooltip.id);
    expect(errorTooltip).toHaveTextContent("Error · Server error");
    expect(errorTooltip).toHaveTextContent(providerError.primary!.message);

    fireEvent.pointerLeave(errorCell);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    rerender(<RequestStatus status={null} state="completed" assessment={disconnectWarning} />);
    const warningCell = screen.getByRole("img", { name: /Request warning: Disconnected/ });
    expect(screen.getByText("Disconnected")).toBeInTheDocument();
    expect(screen.queryByText("No response")).not.toBeInTheDocument();
    expect(screen.queryByText(disconnectWarning.primary!.message)).not.toBeInTheDocument();
    expect(within(showTooltip(warningCell)).getByText("Warning")).toBeInTheDocument();

    fireEvent.scroll(window);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("shows a restated HTTP status as a plain cell with nothing to explain", () => {
    const http401: RequestAssessment = {
      level: "error",
      primary: { source: "http", kind: "http_401", message: "Upstream returned HTTP 401" },
      issue_count: 1,
    };
    render(<RequestStatus status={401} state="completed" assessment={http401} />);
    expect(screen.getByText("401")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("opens headline issue tooltips", () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <RecordHeadlineStatus response={response} state="completed" assessment={providerError} />,
    );
    expect(screen.getByLabelText("HTTP/2 200 OK")).toBeInTheDocument();
    expect(screen.getByText("HTTP/2")).toBeInTheDocument();
    expect(screen.getByText("Server error")).toBeInTheDocument();
    expect(screen.getByText("+1")).toBeInTheDocument();

    rerender(
      <RecordHeadlineStatus response={null} state="completed" assessment={disconnectWarning} />,
    );
    // The failure kind is the whole status: no grey "No response" beside it.
    expect(screen.queryByLabelText("No response")).not.toBeInTheDocument();
    const warningTag = screen.getByText("Disconnected");

    const tooltip = showTooltip(warningTag);
    expect(tooltip).toHaveTextContent("Warning · Disconnected");
    expect(tooltip).toHaveTextContent(disconnectWarning.primary!.message);

    fireEvent.pointerLeave(warningTag);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("opens headline diagnostics with focus or touch and closes them with Escape", () => {
    render(
      <RecordHeadlineStatus response={null} state="completed" assessment={disconnectWarning} />,
    );
    const trigger = screen.getByRole("button", { name: /Warning: Disconnected/ });

    fireEvent.keyDown(document, { key: "Tab" });
    fireEvent.focus(trigger);
    expect(screen.getByRole("tooltip")).toHaveTextContent(disconnectWarning.primary!.message);
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();

    fireEvent.click(trigger);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
  });

  it("opens compact diagnostics on touch-style click and closes outside", () => {
    render(<RequestStatus status={null} state="completed" assessment={disconnectWarning} />);
    const trigger = screen.getByRole("img", { name: /Disconnected/ });

    fireEvent.click(trigger);
    expect(screen.getByRole("tooltip")).toHaveTextContent(disconnectWarning.primary!.message);

    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("renders Waiting before response metadata arrives", () => {
    render(<RecordHeadlineStatus response={null} state="active" assessment={active} />);
    expect(screen.getByText("Waiting")).toBeInTheDocument();
    expect(screen.queryByText("No response")).not.toBeInTheDocument();
  });
});
