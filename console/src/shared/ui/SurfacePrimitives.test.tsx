import { render, screen } from "@testing-library/react";
import { AlertTriangle } from "lucide-react";
import { describe, expect, it } from "vitest";
import { AlertBanner, SectionHeader } from "@/shared/ui/SurfacePrimitives";

describe("Surface primitives", () => {
  it("keeps semantic heading and action structure", () => {
    render(
      <SectionHeader
        eyebrow="Operational status"
        title="Key facts"
        action={<button type="button">Refresh</button>}
      />,
    );
    expect(screen.getByRole("heading", { level: 2, name: "Key facts" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeInTheDocument();
  });

  it("maps alert tones to appropriate live semantics", () => {
    const { rerender } = render(
      <AlertBanner icon={<AlertTriangle />} tone="danger">
        Failed
      </AlertBanner>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Failed");
    rerender(<AlertBanner tone="warning">Partial</AlertBanner>);
    expect(screen.getByRole("status")).toHaveTextContent("Partial");
  });
});
