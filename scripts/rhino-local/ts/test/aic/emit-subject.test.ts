import { describe, expect, it } from "vitest";
import { instrumentSubject } from "../../src/aic/emit-subject.ts";
import { lintAmScript } from "./helpers.ts";

describe("instrumentSubject", () => {
  it("keeps the author source byte-for-byte at top level between snapshots", () => {
    const author = 'var value = nodeState.get("username");\naction.goTo("true");\n';
    const emitted = instrumentSubject(author, "test01");
    const start = emitted.source.indexOf("// rhino-local author source begins\n");
    const end = emitted.source.indexOf("// rhino-local author source ends");
    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    expect(
      emitted.source.slice(
        start + "// rhino-local author source begins\n".length,
        end
      )
    ).toBe(`${author}\n`);
    expect(emitted.source.indexOf(".before =")).toBeLessThan(start);
    expect(emitted.source.indexOf(".final =")).toBeGreaterThan(end);
  });

  it("uses one collision-checked top-level binding and writes state only afterwards", () => {
    const author = [
      "var __rhinoLocalHarness_test01 = 'author';",
      "var marker = '__rhino_local_snapshot_test01';",
      'action.goTo("true");',
    ].join("\n");
    const emitted = instrumentSubject(author, "test01");
    expect(emitted.bindingName).toBe("__rhinoLocalHarness_test01_");
    expect(emitted.snapshotKey).toBe("__rhino_local_snapshot_test01_");
    expect(author).not.toContain(emitted.bindingName);
    expect(author).not.toContain(emitted.snapshotKey);
    const prefix = emitted.source.slice(0, emitted.source.indexOf(author));
    expect(prefix.match(/^var /gm)).toHaveLength(1);
    expect(prefix).not.toContain("putTransient");
    expect(prefix).not.toContain("putShared");
    expect(emitted.source.indexOf("putTransient")).toBeGreaterThan(
      emitted.source.indexOf(author) + author.length
    );
  });

  it("passes AM Rhino lint without wrapping the author source", async () => {
    const emitted = instrumentSubject('action.goTo("true");\n', "lint");
    expect(await lintAmScript(emitted.source, "generated/aic-subject.cjs")).toEqual([]);
  });
});
