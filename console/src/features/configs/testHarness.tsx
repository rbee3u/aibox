import type { ComponentProps } from "react";
import { vi } from "vitest";
import type { ConfigApi, ConfigFileData, ConfigListData } from "@/api/configs";
import type { Bootstrap } from "@/api/core";
import { ConfigPage as ConfigPageView } from "@/features/configs/ConfigPage";
import { TENANT_ROWS as tenants } from "@/features/common/testFixtures";
import { useTestLocation } from "@/test/useTestLocation";

export function ConfigPage(
  props: Omit<ComponentProps<typeof ConfigPageView>, "api" | "search" | "onLocationChange"> & {
    api: ConfigApi;
    onLocationChange?: ComponentProps<typeof ConfigPageView>["onLocationChange"];
  },
) {
  const { api, onLocationChange: notify, ...pageProps } = props;
  const location = useTestLocation(undefined, notify);
  return (
    <ConfigPageView
      api={api}
      search={location.currentSearch}
      onLocationChange={location.onLocationChange}
      {...pageProps}
    />
  );
}

export function configApi(
  options: {
    bootstrap?: Partial<Bootstrap>;
    listTenants?: ConfigApi["listTenants"];
    listConfigs?: ConfigApi["listConfigs"];
    revealConfigFile?: ConfigApi["revealConfigFile"];
    diagnoseConfigFile?: ConfigApi["diagnoseConfigFile"];
    saveConfigFile?: ConfigApi["saveConfigFile"];
    createConfig?: ConfigApi["createConfig"];
    applyConfig?: ConfigApi["applyConfig"];
    deleteConfigs?: ConfigApi["deleteConfigs"];
    previewCredentialPropagation?: ConfigApi["previewCredentialPropagation"];
    executeCredentialPropagation?: ConfigApi["executeCredentialPropagation"];
  } = {},
) {
  const unconfigured = (operation: string) =>
    Promise.reject(new Error(`${operation} was not configured for this test`));
  const defaultCatalog = {
    configs: [],
    files: ["config.toml", "auth.json"],
    application: { last_application: null, drift: "untracked" },
    credential_propagation_available: false,
  } satisfies ConfigListData;
  const defaultFile = {
    file: "config.toml",
    exists: false,
    revision: "missing",
    content_base64: "",
  } satisfies ConfigFileData;
  const listTenants = vi.fn<ConfigApi["listTenants"]>(
    options.listTenants ?? (() => Promise.resolve(tenants)),
  );
  const listConfigs = vi.fn<ConfigApi["listConfigs"]>(
    options.listConfigs ?? (() => Promise.resolve(defaultCatalog)),
  );
  const revealConfigFile = vi.fn<ConfigApi["revealConfigFile"]>(
    options.revealConfigFile ?? (() => Promise.resolve(defaultFile)),
  );
  const diagnoseConfigFile = vi.fn<ConfigApi["diagnoseConfigFile"]>(
    options.diagnoseConfigFile ?? (() => unconfigured("diagnoseConfigFile")),
  );
  const saveConfigFile = vi.fn<ConfigApi["saveConfigFile"]>(
    options.saveConfigFile ?? (() => unconfigured("saveConfigFile")),
  );
  const createConfig = vi.fn<ConfigApi["createConfig"]>(
    options.createConfig ?? (() => unconfigured("createConfig")),
  );
  const applyConfig = vi.fn<ConfigApi["applyConfig"]>(
    options.applyConfig ?? (() => unconfigured("applyConfig")),
  );
  const deleteConfigs = vi.fn<ConfigApi["deleteConfigs"]>(
    options.deleteConfigs ?? (() => unconfigured("deleteConfigs")),
  );
  const previewCredentialPropagation = vi.fn<ConfigApi["previewCredentialPropagation"]>(
    options.previewCredentialPropagation ??
      (() => Promise.reject(new Error("No propagation preview configured"))),
  );
  const executeCredentialPropagation = vi.fn<ConfigApi["executeCredentialPropagation"]>(
    options.executeCredentialPropagation ??
      (() => Promise.reject(new Error("No propagation report configured"))),
  );
  const api = {
    bootstrap: {
      version: options.bootstrap?.version ?? "test",
      csrf_token: options.bootstrap?.csrf_token ?? "token",
      listen: options.bootstrap?.listen ?? "127.0.0.1:3000",
    },
    listTenants,
    listConfigs,
    revealConfigFile,
    diagnoseConfigFile,
    saveConfigFile,
    createConfig,
    applyConfig,
    deleteConfigs,
    previewCredentialPropagation,
    executeCredentialPropagation,
  } satisfies ConfigApi;
  return {
    api,
    listTenants,
    listConfigs,
    revealConfigFile,
    diagnoseConfigFile,
    saveConfigFile,
    createConfig,
    applyConfig,
    deleteConfigs,
    previewCredentialPropagation,
    executeCredentialPropagation,
  };
}
