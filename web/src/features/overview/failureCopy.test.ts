import { describe, expect, it } from "vitest";
import { explainFailure } from "@/features/overview/failureCopy";

describe("explainFailure", () => {
  it("tells a dead daemon apart from a missing binary, because the fix differs", () => {
    // The real text when `docker` is absent from PATH.
    const missing = explainFailure(
      "docker",
      "inspect docker image metadata (is docker installed?): No such file or directory (os error 2)",
    );
    expect(missing.detail).toBe("Docker was not found. Install Docker, then refresh.");

    // The real text when the CLI runs but the daemon is down: the Service tries
    // `image inspect`, falls back to `image ls`, and reports both failures.
    const daemon = explainFailure(
      "docker",
      "docker image inspect failed (exit status: 1): Cannot connect to the Docker daemon at " +
        "unix:///var/run/docker.sock. Is the docker daemon running?; docker image ls failed " +
        "(exit status: 1): Cannot connect to the Docker daemon at unix:///var/run/docker.sock. " +
        "Is the docker daemon running?",
    );
    expect(daemon.detail).toBe("The Docker daemon is not running. Start Docker, then refresh.");
  });
  it("prefers the daemon reading when a failed inspect also mentions a missing socket file", () => {
    const both = explainFailure(
      "docker",
      "inspect docker image metadata (is docker installed?): Cannot connect to the Docker " +
        "daemon at unix:///var/run/docker.sock: No such file or directory",
    );
    expect(both.detail).toBe("The Docker daemon is not running. Start Docker, then refresh.");
  });
  it("names the two failures that are not Docker's", () => {
    expect(explainFailure("service", "Failed to fetch").detail).toBe(
      "The AIBox Service could not be reached. It may have stopped.",
    );
    expect(
      explainFailure("topology", "walk tenant catalog: Permission denied (os error 13)").detail,
    ).toBe("AIBox is not allowed to read this. Check the permissions on $AIBOX_ROOT.");
  });
  it("states what failed without guessing why when the cause is unrecognized", () => {
    const cases = [
      ["docker", "Docker is unavailable."],
      ["service", "Service status could not be read."],
      ["topology", "Tenant resources could not be inspected."],
      ["build", "The Runtime Image build failed."],
    ] as const;
    for (const [source, expected] of cases) {
      expect(explainFailure(source, "docker build failed (exit status: 7)").detail).toBe(
        source === "docker" ? "Docker is unavailable." : expected,
      );
    }
  });
  it("keeps the raw text as evidence, and offers none when there was none", () => {
    const raw = "docker image inspect failed (exit status: 1): something specific";
    expect(explainFailure("docker", raw).technical).toBe(raw);
    expect(explainFailure("docker", null).technical).toBeUndefined();
    expect(explainFailure("docker", "   ").technical).toBeUndefined();
    expect(explainFailure("docker", null).detail).toBe("Docker is unavailable.");
  });
});
