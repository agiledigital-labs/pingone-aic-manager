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

  it("uses one collision-checked top-level binding, and seeds only through it", () => {
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
    // The seed now runs in the subject, so state writes DO precede the author
    // source. What must stay true is that every one of them goes through the
    // harness binding's seed helper: a bare top-level `nodeState.putShared`
    // would be indistinguishable from something the author wrote.
    for (const line of prefix.split("\n")) {
      if (line.includes("nodeState.put")) {
        expect(line.trim()).toMatch(/^nodeState\.put(Shared|Transient)\(k, v\);$/);
      }
    }
    expect(prefix).toContain(`${emitted.bindingName}.seed(`);
    // The snapshot write still lands after the author source.
    expect(emitted.source.indexOf(emitted.snapshotKey, emitted.source.indexOf(author))).toBeGreaterThan(
      emitted.source.indexOf(author) + author.length
    );
  });

  it("seeds shared and transient state ahead of the before-snapshot", () => {
    const emitted = instrumentSubject('action.goTo("true");\n', "seed01", {
      sharedState: { username: "alice" },
      transientState: { attempt: 2 },
    });
    // The seed is a JSON string handed to JSON.parse, so the keys appear
    // escaped inside a JS string literal rather than as bare identifiers.
    expect(emitted.source).toContain(String.raw`\"username\":\"alice\"`);
    expect(emitted.source).toContain(String.raw`\"attempt\":2`);
    // Ordering is the point: a seed applied after the snapshot would show up
    // as state the script added, so every seeded key would read as a mutation.
    expect(emitted.source.indexOf(".seed(")).toBeLessThan(
      emitted.source.indexOf(".before =")
    );
  });

  it("seeds nothing when the case seeds nothing", () => {
    const emitted = instrumentSubject('action.goTo("true");\n', "bare");
    expect(emitted.source).toContain('.seed(JSON.parse("{}")');
  });

  it("passes AM Rhino lint without wrapping the author source", async () => {
    const emitted = instrumentSubject('action.goTo("true");\n', "lint");
    expect(await lintAmScript(emitted.source, "generated/aic-subject.cjs")).toEqual([]);
  });
});
