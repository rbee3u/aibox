import { describe, expect, it } from "vitest";
import type { ConfigCatalogEntry } from "@/api/configs";
import {
  appliedConfigPresentation,
  comparableProvider,
  configIssuePresentation,
  configWarningPresentation,
  driftCatalogLabel,
  lastAppliedMeta,
  propagationDetail,
  propagationGroup,
  propagationGroups,
  propagationStatus,
  proxyValueIsValid,
  requestProxyRoute,
  splitRequestProxyValue,
} from "@/features/configs/configCatalog";

function entry(overrides: Partial<ConfigCatalogEntry> = {}): ConfigCatalogEntry {
  return { name: "review", state: "ready", ...overrides };
}

describe("Last Application metadata", () => {
  it("names the recorded source without calling it Active", () => {
    expect(lastAppliedMeta("openai")).toBe("Last applied openai");
    expect(lastAppliedMeta("openai", "clean")).toBe("Last applied openai");
  });

  it("says the Current Config differs without a Dirty badge", () => {
    expect(lastAppliedMeta("openai", "dirty")).toBe("Last applied openai · differs");
  });
});

describe("Named Config drift labels", () => {
  it("uses Differs for dirty drift instead of Dirty", () => {
    expect(driftCatalogLabel("dirty")).toBe("Differs");
    expect(driftCatalogLabel("clean")).toBe("Clean");
    expect(driftCatalogLabel("untracked")).toBe("Untracked");
    expect(driftCatalogLabel("source-missing")).toBe("Source missing");
    expect(driftCatalogLabel("comparison-error")).toBe("Comparison error");
  });
});

describe("Named Config issue presentation", () => {
  it("stays silent for a ready Config without warnings", () => {
    expect(configIssuePresentation(entry())).toBeNull();
    expect(configWarningPresentation(entry())).toBeNull();
  });

  it("treats incomplete as a repairable warning and invalid as an error", () => {
    expect(configIssuePresentation(entry({ state: "incomplete" }))).toMatchObject({
      tone: "warning",
      label: "Incomplete Config",
    });
    expect(configIssuePresentation(entry({ state: "invalid" }))).toMatchObject({
      tone: "error",
      label: "Invalid Config",
    });
  });

  it("prefers the reported detail over the generic explanation", () => {
    expect(
      configIssuePresentation(entry({ state: "invalid", detail: "unsafe entry" }))?.message,
    ).toBe("unsafe entry");
  });

  it("joins warnings for a ready Config", () => {
    expect(configWarningPresentation(entry({ warnings: ["first.", "second."] }))).toMatchObject({
      tone: "warning",
      message: "first. second.",
    });
  });
});

describe("credential propagation outcomes", () => {
  it("groups each outcome by what the reader must do, a failed write first", () => {
    expect(propagationGroups[0]).toBe("failed");
    expect(propagationGroup("failed")).toBe("failed");
    expect(propagationGroup("updated")).toBe("updated");
    for (const status of ["unchanged", "newer"] as const) {
      expect(propagationGroup(status), status).toBe("skipped");
    }
    for (const status of ["conflict", "invalid"] as const) {
      expect(propagationGroup(status), status).toBe("attention");
    }
  });

  it("names every outcome in sentence case with a tone", () => {
    expect(propagationStatus("updated", true)).toEqual({ tone: "good", label: "Will update" });
    expect(propagationStatus("updated", false)).toEqual({ tone: "good", label: "Updated" });
    expect(propagationStatus("newer", false)).toEqual({ tone: "neutral", label: "Target newer" });
    expect(propagationStatus("conflict", false).tone).toBe("warning");
    expect(propagationStatus("failed", false)).toEqual({ tone: "error", label: "Failed" });
  });

  it("explains each outcome as a sentence with formatted times", () => {
    expect(propagationDetail({ status: "updated" })).toBeNull();
    expect(propagationDetail({ status: "updated" }, true)).toBe(
      "Older credentials for the same account",
    );
    expect(propagationDetail({ status: "conflict", last_refresh: "2026-09-10T08:12:00Z" })).toBe(
      "Different content with the same refresh time, 2026-09-10 16:12:00",
    );
    expect(
      propagationDetail({
        status: "newer",
        source_last_refresh: "2026-09-10T08:12:00Z",
        target_last_refresh: "2026-09-12T19:40:00Z",
      }),
    ).toBe("Refreshed 2026-09-13 03:40:00, after the source at 2026-09-10 16:12:00");
    expect(propagationDetail({ status: "failed", reason: "denied" })).toBe("denied");
  });
});

describe("Request Proxy routing", () => {
  it("derives the loopback route for the Host Tenant and the gateway for a Managed one", () => {
    expect(requestProxyRoute({ kind: "host" }, "127.0.0.1:3000")).toBe("http://127.0.0.1:3000/");
    expect(requestProxyRoute({ kind: "managed", name: "work" }, "0.0.0.0:8080")).toBe(
      "http://host.docker.internal:8080/",
    );
  });

  it("reports no route without a usable port", () => {
    expect(requestProxyRoute({ kind: "host" }, undefined)).toBeNull();
    expect(requestProxyRoute({ kind: "host" }, "127.0.0.1:0")).toBeNull();
    expect(requestProxyRoute({ kind: "host" }, "unix:/tmp/aibox.sock")).toBeNull();
  });

  it("splits a routed upstream from its proxy prefix", () => {
    const route = "http://127.0.0.1:3000/";
    expect(splitRequestProxyValue("http://127.0.0.1:3000/https://api.test", route)).toEqual({
      upstream: "https://api.test",
      routed: true,
    });
    expect(splitRequestProxyValue("https://api.test", route)).toEqual({
      upstream: "https://api.test",
      routed: false,
    });
    expect(splitRequestProxyValue("http://127.0.0.1:0/https://api.test", route)).toEqual({
      upstream: "http://127.0.0.1:0/https://api.test",
      routed: false,
    });
  });

  it("accepts only absolute HTTP(S) upstreams", () => {
    expect(proxyValueIsValid("https://api.test/v1")).toBe(true);
    expect(proxyValueIsValid("http://api.test")).toBe(true);
    expect(proxyValueIsValid("ftp://api.test")).toBe(false);
    expect(proxyValueIsValid("/v1/responses")).toBe(false);
    expect(proxyValueIsValid("")).toBe(false);
  });

  it("compares only the provider fields a save would change", () => {
    expect(comparableProvider(undefined)).toBeNull();
    expect(
      comparableProvider({
        included: true,
        name: "local",
        base_url: "http://api.test",
        request_proxy_route: true,
        proxy_routed: true,
      }),
    ).toEqual({ included: true, name: "local", base_url: "http://api.test" });
  });
});

describe("appliedConfigPresentation", () => {
  it("reads Applied with nothing left to apply when the application is clean", () => {
    expect(appliedConfigPresentation({ last_application: null, drift: "clean" })).toEqual({
      label: "Applied",
      tone: "good",
      variant: "inline",
      applicable: false,
    });
  });
  it("keeps the shared drift label and an applicable Apply for every other state", () => {
    expect(appliedConfigPresentation({ last_application: null, drift: "dirty" })).toEqual({
      label: "Differs",
      tone: "warning",
      variant: "badge",
      applicable: true,
    });
    expect(
      appliedConfigPresentation({ last_application: null, drift: "comparison-error" }).label,
    ).toBe("Comparison error");
  });
});
