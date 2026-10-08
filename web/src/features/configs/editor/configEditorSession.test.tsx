import { useSyncExternalStore } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConfigFilePane } from "@/features/configs/editor/ConfigFilePane";
import { describe, expect, it, vi } from "vitest";
import type { ConfigApi, ConfigFileData } from "@/api/configs";
import { configFile } from "@/features/configs/testFixtures";
import { ConfigEditorSession } from "@/features/configs/editor/configEditorSession";
import { deferred } from "@/test/deferred";

function fixture(
  overrides: Partial<
    Pick<ConfigApi, "revealConfigFile" | "saveConfigFile" | "diagnoseConfigFile">
  > = {},
) {
  const api = {
    bootstrap: { version: "test", csrf_token: "test", listen: "127.0.0.1:9923" },
    revealConfigFile: vi.fn<ConfigApi["revealConfigFile"]>(
      overrides.revealConfigFile ?? (({ file }) => Promise.resolve(configFile(file, "original"))),
    ),
    saveConfigFile: vi.fn<ConfigApi["saveConfigFile"]>(
      overrides.saveConfigFile ?? (() => Promise.reject(new Error("unexpected save"))),
    ),
    diagnoseConfigFile: vi.fn<ConfigApi["diagnoseConfigFile"]>(
      overrides.diagnoseConfigFile ?? (() => Promise.resolve({ diagnostics: [] })),
    ),
  };
  const onError = vi.fn();
  const onSaved = vi.fn();
  const session = new ConfigEditorSession(
    api,
    {
      tenant: { kind: "host" },
      agent: "codex",
      current: true,
      config: null,
    },
    ["auth.json", "config.toml"],
    onError,
    onSaved,
  );
  return { session, api, onError, onSaved };
}

describe("Config editing scope", () => {
  it("commits sequential saves immediately and retains earlier success after a later failure", async () => {
    const { session, api } = fixture({
      saveConfigFile: ({ file }, input) =>
        file === "auth.json"
          ? Promise.resolve({ ...configFile(file, ""), content_base64: input.contentBase64 })
          : Promise.reject(new Error("main save refused")),
    });
    session.start();
    try {
      await vi.waitFor(() => expect(session.file("config.toml").loading).toBe(false));
      session.updateEditor("auth.json", "new credentials");
      session.updateEditor("config.toml", "new main");
      expect(await session.saveInOrder(["auth.json", "config.toml"])).toBe(false);
      expect(api.saveConfigFile.mock.calls.map(([target]) => target.file)).toEqual([
        "auth.json",
        "config.toml",
      ]);
      expect(session.file("auth.json").dirty).toBe(false);
      expect(session.file("auth.json").editor).toBe("new credentials");
      expect(session.dirtyFiles).toEqual(["config.toml"]);
      expect(session.file("config.toml").saveFailure?.message).toBe("main save refused");
    } finally {
      session.stop();
    }
  });

  it("ignores reads and saves from a stopped scope and resets pending feedback on reload", async () => {
    const read = deferred<ConfigFileData>();
    const save = deferred<ConfigFileData>();
    const { session, api, onSaved } = fixture({
      revealConfigFile: ({ file }) =>
        file === "auth.json" ? read.promise : Promise.resolve(configFile(file, "original")),
      saveConfigFile: () => save.promise,
    });
    session.start();
    await vi.waitFor(() => expect(session.file("config.toml").loading).toBe(false));
    session.updateEditor("config.toml", "draft");
    const pending = session.save("config.toml");
    expect(session.file("config.toml").feedback).toBe("saving");
    session.stop();
    api.revealConfigFile.mockImplementation(({ file }) =>
      Promise.resolve(configFile(file, "fresh")),
    );
    session.start();
    try {
      await vi.waitFor(() => expect(session.file("auth.json").editor).toBe("fresh"));
      expect(session.file("config.toml").feedback).toBe("idle");
      read.resolve(configFile("auth.json", "stale"));
      save.resolve(configFile("config.toml", "stale saved"));
      expect(await pending).toBe(false);
      expect(session.file("auth.json").editor).toBe("fresh");
      expect(session.file("config.toml").editor).toBe("fresh");
      expect(onSaved).not.toHaveBeenCalled();
    } finally {
      session.stop();
    }
  });

  it("invalidates comparison on an edit round trip, but not on diagnostics", async () => {
    vi.useFakeTimers();
    const diagnosis = deferred<Awaited<ReturnType<ConfigApi["diagnoseConfigFile"]>>>();
    const { session } = fixture({ diagnoseConfigFile: () => diagnosis.promise });
    session.start();
    try {
      await Promise.resolve();
      const original = session.getSnapshot().revision;
      session.updateEditor("config.toml", "changed");
      session.updateEditor("config.toml", "original");
      const revised = session.getSnapshot().revision;
      expect(revised).toBeGreaterThan(original);
      expect(session.dirtyFiles).toEqual([]);
      await vi.advanceTimersByTimeAsync(250);
      diagnosis.resolve({
        diagnostics: [{ message: "example", line: 1, column: 1, severity: "error" }],
      });
      await Promise.resolve();
      expect(session.getSnapshot().revision).toBe(revised);
      expect(session.file("config.toml").rawDiagnostics).toHaveLength(1);
      session.stop();
      session.start();
      await Promise.resolve();
      expect(session.file("config.toml").rawDiagnostics).toEqual([]);
    } finally {
      session.stop();
    }
  });
});

it("keeps drafts when a file view unmounts and remounts within its editing scope", async () => {
  const { session } = fixture();
  function FileView({ visible }: { visible: boolean }) {
    const state = useSyncExternalStore(session.subscribe, session.getSnapshot);
    return visible ? (
      <ConfigFilePane
        tenant={session.target.tenant}
        file="config.toml"
        mode={state.mode}
        controlsDisabled={false}
        session={session}
        data={session.file("config.toml")}
        onRequestRaw={session.showRawEditor}
      />
    ) : null;
  }
  session.start();
  try {
    const user = userEvent.setup();
    const view = render(<FileView visible />);
    const input = await screen.findByRole("textbox", { name: "config.toml content" });
    await user.clear(input);
    await user.type(input, "unsaved draft");
    view.rerender(<FileView visible={false} />);
    view.rerender(<FileView visible />);
    expect(screen.getByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "unsaved draft",
    );
    expect(session.dirtyFiles).toEqual(["config.toml"]);
  } finally {
    session.stop();
  }
});
