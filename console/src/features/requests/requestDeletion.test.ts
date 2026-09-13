import { describe, expect, it } from "vitest";

import { requestDeletionFacts } from "@/features/requests/requestDeletion";
import { completedSummary } from "@/features/requests/testFixtures";

describe("requestDeletionFacts", () => {
  it("restates the catalog row for a completed Request", () => {
    expect(requestDeletionFacts(completedSummary, completedSummary.id)).toEqual([
      { label: "Request", value: "POST api.example.test/v1/responses" },
      { label: "Status", value: "200" },
      { label: "Ended", value: "2026-08-06 12:00:01" },
      { label: "Id", value: "0198-demo-completed" },
    ]);
  });

  it("uses Started and No response when the record has no HTTP response", () => {
    expect(
      requestDeletionFacts(
        {
          ...completedSummary,
          id: "0198-demo-disconnected",
          status: null,
          ended_at: null,
          state: "completed",
        },
        "0198-demo-disconnected",
      ),
    ).toEqual([
      { label: "Request", value: "POST api.example.test/v1/responses" },
      { label: "Status", value: "No response" },
      { label: "Started", value: "2026-08-06 12:00:00" },
      { label: "Id", value: "0198-demo-disconnected" },
    ]);
  });

  it("keeps the id when the catalog row is no longer on the page", () => {
    expect(requestDeletionFacts(undefined, "0198-demo-missing")).toEqual([
      { label: "Id", value: "0198-demo-missing" },
    ]);
  });
});
