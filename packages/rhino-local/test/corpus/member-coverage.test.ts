import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { realCases } from "../../cases/real/index.ts";
import { bindingsJsonPath, repoRoot } from "../../src/paths.ts";
import {
  membersOf,
  type ContextsDocument,
  type Element,
} from "../../src/schema.ts";

/**
 * Every member of the scripted-decision binding surface is exercised by at
 * least one corpus case's origin fixture — the scripts whose live behaviour is
 * committed — or is excluded here with its reason. A binding member no case
 * runs is one the mocks can get wrong with every gate green.
 *
 * Textual, so deliberately strict about what counts: comments are stripped
 * (a fixture saying "emailService.send is not called" is not a call), and a
 * method needs a call — `path(` (whitespace allowed around the dots), or
 * `alias.member(` where the fixture assigned `var alias = parent;`, or
 * `parent[NAMES[i]](` where the array literal `var NAMES = [ … ]` quotes the
 * member. A quoted name and a subscript that are not the same call do not
 * count: `typeof callbacksBuilder[candidates[i]]` only enumerates.
 */
const EXCLUDED: Record<string, string> = {
  "emailService.send":
    "sends real mail from the tenant; never called by a probe",
  "action.suspend":
    "suspends the live journey; a probe could not return its payload",
  "callbacksBuilder.httpCallback":
    "AM cannot render it as REST JSON (400 for the whole response); measured in binding-callbacks-http, which has no payload for a case",
  "callbacksBuilder.x509CertificateCallback":
    "as httpCallback; measured in binding-callbacks-x509",
};

function withoutComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
}

function escape(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** `a.b.c` as a pattern that allows `a\n  .b` line breaks. */
function dotted(path: string): string {
  return path.split(".").map(escape).join("\\s*\\.\\s*");
}

function covered(source: string, path: string, isMethod: boolean): boolean {
  const call = isMethod ? "\\s*\\(" : "\\b";
  if (new RegExp(`${dotted(path)}${call}`).test(source)) {
    return true;
  }
  const dot = path.lastIndexOf(".");
  const parent = path.slice(0, dot);
  const member = path.slice(dot + 1);
  // `parent[NAMES[i]](…)`, where `var NAMES = [ … ]` lists the member.
  const subscripted = new RegExp(
    `${dotted(parent)}\\s*\\[\\s*(\\w+)\\s*\\[\\s*\\w+\\s*\\]\\s*\\]\\s*\\(`,
    "g",
  );
  for (const call of source.matchAll(subscripted)) {
    const list = new RegExp(`var\\s+${call[1]}\\s*=\\s*\\[([^\\]]*)\\]`).exec(
      source,
    );
    if (
      list?.[1] !== undefined &&
      new RegExp(`["']${escape(member)}["']`).test(list[1])
    ) {
      return true;
    }
  }
  const aliases = [
    ...source.matchAll(
      new RegExp(`var\\s+(\\w+)\\s*=\\s*${dotted(parent)}\\s*;`, "g"),
    ),
  ];
  return aliases.some((alias) =>
    new RegExp(`\\b${alias[1]}\\s*\\.\\s*${escape(member)}${call}`).test(
      source,
    ),
  );
}

describe("binding member coverage", () => {
  const doc = JSON.parse(
    readFileSync(bindingsJsonPath, "utf8"),
  ) as ContextsDocument;
  const source = withoutComments(
    [...new Set(realCases.map((entry) => entry.origin))]
      .map((origin) => readFileSync(join(repoRoot, origin), "utf8"))
      .join("\n"),
  );
  const paths: { path: string; method: boolean }[] = [];
  function walk(prefix: string, elements: Element[]): void {
    for (const member of membersOf(elements)) {
      const path = `${prefix}.${member.name}`;
      if (member.kind === "field" && member.elements.length > 0) {
        walk(path, member.elements);
      } else {
        paths.push({ path, method: member.kind === "method" });
      }
    }
  }
  for (const binding of doc.bindings) {
    if (binding.elements?.length) {
      walk(binding.name, binding.elements);
    } else {
      paths.push({ path: binding.name, method: false });
    }
  }

  it("covers every member a corpus case runs, or names why not", () => {
    const missing = paths
      .filter(({ path }) => !(path in EXCLUDED))
      .filter(({ path, method }) =>
        method || path.includes(".")
          ? !covered(source, path, method)
          : !new RegExp(`\\b${escape(path)}\\b`).test(source),
      )
      .map(({ path }) => path);
    expect(missing).toEqual([]);
  });

  it("excludes only members that exist and that no case runs", () => {
    for (const path of Object.keys(EXCLUDED)) {
      const entry = paths.find((p) => p.path === path);
      expect(entry, `${path} is not a binding member`).toBeDefined();
      expect(
        covered(source, path, entry?.method ?? false),
        `${path} is now exercised; drop its exclusion`,
      ).toBe(false);
    }
  });
});
