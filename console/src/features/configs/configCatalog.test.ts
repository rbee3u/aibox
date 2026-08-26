import { describe, expect, it } from "vitest";
import type { ConfigCatalogEntry } from "@/api/configs";
import {
  comparableProvider,
  configIssuePresentation,
  configWarningPresentation,
  propagationDetail,
  propagationGroup,
  proxyValueIsValid,
  requestProxyRoute,
  splitRequestProxyValue,
} from "@/features/configs/configCatalog";

function entry(overrides: Partial<ConfigCatalogEntry> = {}): ConfigCatalogEntry {
  return { name: "review", state: "ready", ...overrides };
}

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
  it("groups each outcome by what the reader must do", () => {
    expect(propagationGroup("updated")).toBe("updated");
    expect(propagationGroup("unchanged")).toBe("skipped");
    for (const status of ["conflict", "newer", "invalid", "failed"] as const) {
      expect(propagationGroup(status), status).toBe("attention");
    }
  });

  it("explains only the outcomes that carry evidence", () => {
    expect(propagationDetail({ status: "updated" })).toBeNull();
    expect(propagationDetail({ status: "conflict", last_refresh: "t1" })).toBe("last refresh t1");
    expect(
      propagationDetail({ status: "newer", source_last_refresh: "t2", target_last_refresh: "t3" }),
    ).toBe("source t2 · target t3");
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
