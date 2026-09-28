import { join } from "node:path";
import { ESLint } from "eslint";
import type { TenantProvider } from "../../src/aic/provider.ts";
import type { TenantSession } from "../../src/aic/tenant.ts";
import { amRhinoEslintConfigPath, packageRoot } from "../../src/paths.ts";
import { makeCase } from "../case/helpers.ts";
import type { Case, Expect, Given } from "../../src/case/index.ts";

export { makeCase };

export function caseWith(
  overrides: {
    name?: string;
    script?: string;
    outcomes?: readonly string[];
    given?: Given;
    expect?: Expect;
  } = {}
): Case {
  return makeCase(overrides);
}

export function sequentialIds(prefix = "00000000-0000-4000-8000-"): () => string {
  let n = 0;
  return () => {
    n += 1;
    return `${prefix}${String(n).padStart(12, "0")}`;
  };
}

export async function lintAmScript(source: string, filePath: string): Promise<string[]> {
  const eslint = new ESLint({
    cwd: packageRoot,
    overrideConfigFile: amRhinoEslintConfigPath,
  });
  const results = await eslint.lintText(source, {
    filePath: join(packageRoot, filePath),
  });
  const lines: string[] = [];
  for (const result of results) {
    for (const message of result.messages) {
      lines.push(`${filePath}:${message.line} ${message.message}`);
    }
  }
  return lines;
}
/** Stable provider for tests that exercise tenant work, not token refresh. */
export function stubTenantSession(overrides: Partial<TenantSession> = {}): TenantSession {
  const provider: TenantProvider = {
    describe: async () => ({ name: "sandbox", baseUrl: "https://tenant.example.com" }),
    getToken: async () => "test-token",
  };
  return {
    tenantName: "sandbox",
    baseUrl: "https://tenant.example.com",
    token: "test-token",
    provider,
    ...overrides,
  };
}
