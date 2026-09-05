import { describe, expect, it } from "vitest";

import type { OverviewData } from "@/api/overview";
import { formatStatusSummary } from "@/features/overview/components/statusSummary";

const overview = {
  service: {
    version: "1.2.3",
    listen: "127.0.0.1:8080",
    uptime_seconds: 90,
    aibox_root: "/home/test/.aibox",
  },
  docker: { status: "available", error: null },
  runtime_image: {
    reference: "aibox:test",
    status: "built",
    id: "sha256:0123456789abcdef",
    created_at: "2026-08-18T01:02:03Z",
    size_bytes: 4_194_304,
    detail: null,
  },
  managed_tenants: 3,
  host_available: true,
  host_home: "/home/test",
} satisfies OverviewData;

describe("formatStatusSummary", () => {
  it("joins the same health facts and keeps metadata on the title", () => {
    expect(formatStatusSummary(overview, null)).toEqual({
      line: "Running · 3 Managed · Host available · Docker available · Image built",
      detail: "1.2.3 · 127.0.0.1:8080 · ~/.aibox",
    });
  });

  it("names host, Docker, and image trouble without inventing a diagnosis", () => {
    expect(
      formatStatusSummary(
        {
          ...overview,
          host_available: false,
          docker: { status: "unavailable", error: "Docker is not running" },
          runtime_image: { ...overview.runtime_image, status: "missing", id: null },
        },
        null,
      ).line,
    ).toBe("Running · 3 Managed · Host unavailable · Docker unavailable · Image missing");
  });

  it("uses the service error or a loading placeholder before overview arrives", () => {
    expect(formatStatusSummary(null, "control API failed")).toEqual({
      line: "Unavailable",
      detail: "control API failed",
    });
    expect(formatStatusSummary(null, null)).toEqual({
      line: "Loading",
      detail: "Connecting",
    });
  });
});
