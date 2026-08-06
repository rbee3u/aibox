import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { RecordState } from "../types";
import { RecordStatus } from "./RecordStatus";
import styles from "./RecordStatus.module.css";
import { outcomeLabel, recordStatusPresentation } from "./statusPresentation";

function presentation(
  status: number | null,
  outcome = "completed",
  state: RecordState = "completed",
) {
  return recordStatusPresentation({ status, outcome, state });
}

describe("recordStatusPresentation", () => {
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

  it("keeps lifecycle anomalies separate from a successful HTTP status", () => {
    expect(presentation(200, "client_disconnected")).toEqual({
      label: "200",
      tone: "success",
      anomaly: "Client disconnected",
      recording: false,
    });
  });

  it("distinguishes active records from terminal records without a response", () => {
    expect(presentation(null, "active", "active")).toEqual({
      label: "Active",
      tone: "active",
      anomaly: null,
      recording: false,
    });
    expect(presentation(null, "interrupted", "interrupted")).toEqual({
      label: "No response",
      tone: "error",
      anomaly: "Interrupted",
      recording: false,
    });
  });

  it("humanizes unknown outcome names", () => {
    expect(outcomeLabel("future_transport_error")).toBe("Future transport error");
  });
});

describe("RecordStatus", () => {
  it("renders a compact warning with a title and accessible name", () => {
    render(<RecordStatus status={200} outcome="client_disconnected" state="completed" compact />);

    expect(screen.getByText("200")).toHaveClass(styles.success);
    expect(
      screen.getByRole("img", { name: "Record outcome: Client disconnected" }),
    ).toHaveAttribute("title", "Record outcome: Client disconnected");
  });

  it("shows the anomaly reason directly in the detail presentation", () => {
    render(<RecordStatus status={200} outcome="recording_failed" state="completed" />);

    expect(screen.getByText("200")).toHaveClass(styles.success);
    expect(screen.getByText("Recording failed")).toHaveClass(styles.anomaly);
  });

  it("renders terminal records without headers as No response", () => {
    render(<RecordStatus status={null} outcome="upstream_error" state="completed" compact />);

    const status = screen.getByTitle("Record outcome: Upstream error");
    expect(status).toHaveTextContent("No response");
    expect(status).toHaveClass(styles.error);
  });

  it("shows an active status and recording marker when headers have arrived", () => {
    render(<RecordStatus status={200} outcome="active" state="active" compact />);

    expect(screen.getByText("200")).toHaveClass(styles.success);
    expect(screen.getByRole("img", { name: "Recording active traffic" })).toBeInTheDocument();
  });

  it("shows only the active state while waiting for response headers", () => {
    render(<RecordStatus status={null} outcome="active" state="active" compact />);

    expect(screen.getByText("Active")).toHaveClass(styles.active);
    expect(screen.queryByRole("img", { name: "Recording active traffic" })).not.toBeInTheDocument();
  });
});
