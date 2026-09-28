import { readFileSync, readdirSync } from "node:fs";
import { basename, join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { repoRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";
import type { JobResponse } from "../../src/protocol.ts";

const corpusDir = join(repoRoot, "scripts", "rhino-local", "corpus");

interface CorpusHeader {
  row: string;
  verdict: string;
  compiled: string;
  evaluated: string;
  exceptionContains: string;
  result: string;
}

function parseHeader(path: string): CorpusHeader {
  const meta: CorpusHeader = {
    row: basename(path, ".js"),
    verdict: "check",
    compiled: "",
    evaluated: "",
    exceptionContains: "",
    result: "",
  };
  let inHeader = false;
  for (const line of readFileSync(path, "utf8").split(/\r?\n/)) {
    if (!inHeader) {
      if (line.startsWith("// rhino-local-corpus")) {
        inHeader = true;
        continue;
      }
      if (line.startsWith("//")) continue;
      break;
    }
    if (!line.startsWith("//")) break;
    let body = line.slice(2);
    if (body.startsWith(" ")) body = body.slice(1);
    if (body === "" || body.startsWith(" ") || !body.includes(":")) continue;
    const [rawKey, ...valueParts] = body.split(":");
    const value = valueParts.join(":").replace(/^ /, "");
    switch (rawKey?.trim()) {
      case "row":
        meta.row = value;
        break;
      case "verdict":
        meta.verdict = value;
        break;
      case "aic-compiled":
        meta.compiled = value;
        break;
      case "aic-evaluated":
        meta.evaluated = value;
        break;
      case "aic-exception-contains":
        meta.exceptionContains = value;
        break;
      case "aic-result":
        meta.result = value;
        break;
    }
  }
  return meta;
}

function resultAsString(response: JobResponse): string {
  if (response.valueKind === "undefined") return "undefined";
  if (response.valueKind === "missing") return "";
  if (response.value === null) return "null";
  return String(response.value);
}

function matches(response: JobResponse, header: CorpusHeader): boolean | null {
  if (header.verdict !== "check") return null;
  const compiled = response.outcome !== "compile_error";
  const evaluated = response.outcome === "ok";
  if (header.compiled !== "" && compiled !== (header.compiled === "true")) return false;
  if (header.evaluated !== "" && evaluated !== (header.evaluated === "true")) return false;
  if (
    header.exceptionContains !== "" &&
    !(response.error?.message ?? "").includes(header.exceptionContains)
  ) {
    return false;
  }
  if (header.result !== "" && resultAsString(response) !== header.result) return false;
  return true;
}

const files = readdirSync(corpusDir)
  .filter((file) => file.endsWith(".js"))
  .sort()
  .map((file) => join(corpusDir, file));

describe("rhino-local language corpus (VERSION_DEFAULT)", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  for (const file of files) {
    const header = parseHeader(file);
    it(header.row, async () => {
      const response = await runner.eval({
        source: readFileSync(file, "utf8"),
        sourceName: basename(file),
        languageVersion: 0,
        resultGlobal: "__result",
      });
      const expected = matches(response, header);
      if (expected !== null) {
        expect(expected, `${header.row}: ${JSON.stringify(response)}`).toBe(true);
      }
    });
  }
});
