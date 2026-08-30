import { describe, expect, it } from "vitest";
import { initialTenantWorkflow, tenantWorkflowReducer } from "@/features/tenants/tenantWorkflow";

describe("Tenant workflow reducer", () => {
  it("restores only remaining selections after a partial deletion", () => {
    const selected = tenantWorkflowReducer(initialTenantWorkflow, { type: "selection_enter" });
    const deleting = tenantWorkflowReducer(selected, {
      type: "delete_requested",
      names: ["one", "two"],
    });
    const failed = tenantWorkflowReducer(deleting, {
      type: "delete_failed",
      remaining: ["managed:two"],
      resumeSelection: true,
    });

    expect(failed.deleteTarget).toBeNull();
    expect(failed.selectionMode).toBe(true);
    expect([...failed.selectedKeys]).toEqual(["managed:two"]);
    expect(failed.mutationPhase).toBe("idle");
  });

  it("keeps create input on cancel and clears it after success", () => {
    const editing = tenantWorkflowReducer(
      tenantWorkflowReducer(initialTenantWorkflow, { type: "create_open" }),
      { type: "create_name_changed", name: "work" },
    );
    const cancelled = tenantWorkflowReducer(editing, { type: "create_close" });
    const completed = tenantWorkflowReducer(cancelled, { type: "create_succeeded" });

    expect(cancelled.newName).toBe("work");
    expect(completed.newName).toBe("");
    expect(completed.createOpen).toBe(false);
  });
});
