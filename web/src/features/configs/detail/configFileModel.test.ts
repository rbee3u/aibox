import { describe, expect, it } from "vitest";

import type { ConfigCustomProvider, ConfigVisualOption } from "@/api/configs";
import { customProviderPaths, visualSaveFailure } from "@/features/configs/detail/configFileModel";

const route = "http://host.docker.internal:9923/";

function provider(overrides: Partial<ConfigCustomProvider>): ConfigCustomProvider {
  return {
    included: true,
    name: "custom",
    base_url: "https://x",
    request_proxy_route: true,
    proxy_routed: false,
    ...overrides,
  };
}

function field(overrides: Partial<ConfigVisualOption>): ConfigVisualOption {
  return {
    path: "model",
    label: "Model",
    description: "",
    group: "",
    visible_when: null,
    value_kind: "string",
    enum_values: [],
    included: true,
    required: true,
    sensitive: false,
    request_proxy_route: false,
    proxy_routed: false,
    value: "gpt-5",
    ...overrides,
  };
}

describe("visualSaveFailure", () => {
  it("accepts a complete draft", () => {
    expect(
      visualSaveFailure(
        [field({})],
        provider({
          included: true,
          name: "custom",
          base_url: `${route}https://api.example.com/v1`,
        }),
        route,
      ),
    ).toBeNull();
  });
  it("names the Custom provider field that blocks the save", () => {
    expect(
      visualSaveFailure([], provider({ included: true, name: " ", base_url: "https://x" }), route),
    ).toEqual({ message: "Custom provider name is required.", paths: [customProviderPaths.name] });
    expect(
      visualSaveFailure([], provider({ included: true, name: "c", base_url: "" }), route),
    ).toEqual({
      message: "Base URL is required.",
      paths: [customProviderPaths.baseUrl],
    });
    expect(
      visualSaveFailure(
        [],
        provider({ included: true, name: "c", base_url: `${route}not a url` }),
        route,
      ),
    ).toEqual({
      message: "Base URL must be a valid HTTP or HTTPS URL.",
      paths: [customProviderPaths.baseUrl],
    });
    expect(
      visualSaveFailure([], provider({ included: false, name: "", base_url: "" }), route),
    ).toBeNull();
  });
  it("checks only included fields and lets booleans through", () => {
    expect(visualSaveFailure([field({ value: "  " })], null, route)).toEqual({
      message: "Model is required.",
      paths: ["model"],
    });
    expect(visualSaveFailure([field({ value: "", included: false })], null, route)).toBeNull();
    expect(
      visualSaveFailure([field({ value_kind: "bool", value: false, path: "flag" })], null, route),
    ).toBeNull();
    expect(
      visualSaveFailure(
        [field({ path: "env.BASE", label: "Base URL", request_proxy_route: true, value: "ftp:/" })],
        null,
        route,
      ),
    ).toEqual({ message: "Base URL must be a valid HTTP or HTTPS URL.", paths: ["env.BASE"] });
  });
});
