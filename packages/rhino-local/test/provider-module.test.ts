import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { configuredTenantProvider, setTenantProvider } from "../src/aic/provider.ts";
import { loadProviderModule } from "../src/provider-module.ts";

const providerTs = new URL("../src/aic/provider.ts", import.meta.url).href;
let dir: string | undefined;

function module(source: string): string {
  dir ??= mkdtempSync(join(tmpdir(), "rhino-local-provider-module-"));
  const path = join(dir, `m${Math.random().toString(36).slice(2)}.mjs`);
  writeFileSync(path, source);
  return path;
}

afterEach(() => {
  setTenantProvider(undefined);
  if (dir !== undefined) {
    rmSync(dir, { recursive: true, force: true });
    dir = undefined;
  }
});

describe("loadProviderModule", () => {
  it("registers a default-exported provider", async () => {
    await loadProviderModule(
      module(`export default {
        describe: async () => ({ name: "from-default", baseUrl: "https://tenant.example.com" }),
        getToken: async () => "bearer",
      };`)
    );
    expect(await configuredTenantProvider({})?.describe()).toMatchObject({ name: "from-default" });
  });

  it("accepts a module that registers its own through setTenantProvider", async () => {
    await loadProviderModule(
      module(`import { setTenantProvider } from ${JSON.stringify(providerTs)};
        setTenantProvider({
          describe: async () => ({ name: "registered", baseUrl: "https://tenant.example.com" }),
          getToken: async () => "bearer",
        });`)
    );
    expect(await configuredTenantProvider({})?.describe()).toMatchObject({ name: "registered" });
  });

  it("refuses a module that provides nothing", async () => {
    await expect(loadProviderModule(module("export const unrelated = 1;"))).rejects.toThrow(
      /neither default-exports a TenantProvider nor calls setTenantProvider/
    );
  });
});
