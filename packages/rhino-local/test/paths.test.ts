import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import {
  amEslintConfigPath,
  bindingsJsonPath,
  bindingsIdentityPolicyPath,
  buildIdentity,
  buildIdentityPath,
  formatBuildIdentity,
  packageRoot,
  packageVersion,
  sourceBindingsJsonPath,
} from "../src/paths.ts";

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      return sourceFiles(path);
    }
    return entry.name.endsWith(".ts") ? [path] : [];
  });
}

describe("paths", () => {
  it("resolves the shipped scripted-decision contexts snapshot", () => {
    expect(existsSync(bindingsJsonPath)).toBe(true);
    expect(relative(packageRoot, bindingsJsonPath).startsWith("..")).toBe(
      false,
    );
    const parsed: unknown = JSON.parse(readFileSync(bindingsJsonPath, "utf8"));
    expect(parsed).toMatchObject({ _id: "SCRIPTED_DECISION_NODE" });
  });

  it("ships the canonical identity policy at its package-relative path", () => {
    const asset = relative(packageRoot, bindingsIdentityPolicyPath);
    expect(asset.startsWith("..")).toBe(false);
    expect(existsSync(bindingsIdentityPolicyPath)).toBe(true);
    const manifest = JSON.parse(
      readFileSync(join(packageRoot, "package.json"), "utf8"),
    ) as { files: string[] };
    expect(manifest.files).toContain(asset);
  });

  it("resolves the repo's captured contexts JSON and AM ESLint config", () => {
    expect(existsSync(sourceBindingsJsonPath)).toBe(true);
    expect(existsSync(amEslintConfigPath)).toBe(true);
  });

  // An installed package has no repo around it: anything the run-time code
  // reads through repoRoot would be missing in a consumer's node_modules.
  it("formats a packed commit and an unpacked checkout", () => {
    expect(formatBuildIdentity("0.4.0-emulator.2", undefined)).toBe("0.4.0-emulator.2 (commit unknown)");
    expect(formatBuildIdentity("0.4.0-emulator.2", { commit: "abcdef123456", dirty: false })).toBe(
      "0.4.0-emulator.2 (abcdef123456)",
    );
    expect(formatBuildIdentity("0.4.0-emulator.2", { commit: "abcdef123456", dirty: true })).toBe(
      "0.4.0-emulator.2 (abcdef123456-dirty)",
    );
  });

  it("uses the packed identity file when it is present, and never invents a commit", () => {
    expect(packageVersion).toBe("0.4.0-emulator.3");
    if (!existsSync(buildIdentityPath)) {
      expect(buildIdentity).toBe("0.4.0-emulator.3 (commit unknown)");
      return;
    }
    const parsed = JSON.parse(readFileSync(buildIdentityPath, "utf8")) as { commit: string; dirty: boolean };
    expect(buildIdentity).toBe(formatBuildIdentity(packageVersion, parsed));
  });

  it("keeps repo-only paths out of run-time code", () => {
    const allowed = new Set(["paths.ts", "generate.ts"]);
    const offenders = sourceFiles(join(packageRoot, "src"))
      .filter((path) => !allowed.has(relative(join(packageRoot, "src"), path)))
      .filter((path) =>
        /\b(repoRoot|sourceBindingsJsonPath|amEslintConfigPath)\b/.test(
          readFileSync(path, "utf8"),
        ),
      )
      .map((path) => relative(packageRoot, path));
    expect(offenders).toEqual([]);
  });
});
