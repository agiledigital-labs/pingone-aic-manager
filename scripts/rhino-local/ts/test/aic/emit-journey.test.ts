import { describe, expect, it } from "vitest";
import {
  emitWrapperJourney,
  subjectOutcomes,
} from "../../src/aic/emit-journey.ts";
import { SUCCESS_NODE_ID } from "../../src/aic/constants.ts";
import { caseWith, lintAmScript, sequentialIds } from "./helpers.ts";

const SUBJECT = 'action.goTo("true");\n';

describe("subjectOutcomes", () => {
  it("always includes true and false plus expect.outcome", () => {
    expect(subjectOutcomes(caseWith({ expect: { outcome: "true" } }))).toEqual([
      "true",
      "false",
    ]);
    expect(subjectOutcomes(caseWith({ expect: { outcome: "created" } }))).toEqual(
      ["true", "false", "created"]
    );
  });
});

describe("emitWrapperJourney", () => {
  it("wires setup → subject → per-outcome result → success", () => {
    const wrapper = emitWrapperJourney(
      caseWith({
        name: "grants when verified",
        given: { sharedState: { username: "alice" } },
        expect: { outcome: "true" },
      }),
      SUBJECT,
      { runId: "test01", idFactory: sequentialIds() }
    );

    expect(wrapper.treeName).toBe("rl-aic-test01");
    expect(wrapper.realm).toBe("alpha");
    expect(wrapper.scripts.map((script) => script.role)).toEqual([
      "setup",
      "subject",
      "result",
      "result",
    ]);
    expect(wrapper.scripts[1]?.source).toContain(SUBJECT);
    expect(wrapper.scripts[1]?.source).toContain(".before =");
    expect(wrapper.scripts[1]?.source).toContain(".final =");
    expect(wrapper.scripts[1]?.source).toContain("__rhino_local_snapshot_test01");

    const setup = wrapper.nodes.find((node) => node.displayName === "setup");
    const subject = wrapper.nodes.find(
      (node) => node.displayName === "grants when verified"
    );
    const resultTrue = wrapper.nodes.find((node) => node.displayName === "result true");
    const resultFalse = wrapper.nodes.find(
      (node) => node.displayName === "result false"
    );
    expect(setup).toBeDefined();
    expect(subject).toBeDefined();
    expect(resultTrue).toBeDefined();
    expect(resultFalse).toBeDefined();
    if (
      setup === undefined ||
      subject === undefined ||
      resultTrue === undefined ||
      resultFalse === undefined
    ) {
      return;
    }
    expect(setup.connections).toEqual({ true: subject.id });
    expect(subject.connections).toEqual({
      true: resultTrue.id,
      false: resultFalse.id,
    });
    expect(resultTrue.connections).toEqual({ true: SUCCESS_NODE_ID });
    expect(wrapper.treeBody.entryNodeId).toBe(setup.id);
    expect(wrapper.nodeBodies[setup.id]?.inputs).toEqual(["*"]);
    expect(wrapper.nodeBodies[setup.id]?.outputs).toEqual(["*"]);
  });

  it("puts request headers/parameters/cookies on invoke, not in the setup script", () => {
    const wrapper = emitWrapperJourney(
      caseWith({
        given: {
          requestHeaders: { "x-forwarded-for": ["10.0.0.1"] },
          requestParameters: { goto: ["https://example.com/app"] },
          requestCookies: { sid: "abc" },
        },
      }),
      SUBJECT,
      { runId: "hdr", idFactory: sequentialIds() }
    );
    expect(wrapper.invoke).toEqual({
      headers: { "x-forwarded-for": ["10.0.0.1"] },
      parameters: { goto: ["https://example.com/app"] },
      cookies: { sid: "abc" },
    });
    const setup = wrapper.scripts.find((script) => script.role === "setup");
    expect(setup?.source).not.toContain("x-forwarded-for");
  });

  it("uses given.realm for the tree identityResource", () => {
    const wrapper = emitWrapperJourney(
      caseWith({ given: { realm: "bravo" } }),
      SUBJECT,
      { runId: "br", idFactory: sequentialIds() }
    );
    expect(wrapper.realm).toBe("bravo");
    expect(wrapper.identityResource).toBe("managed/bravo_user");
    expect(wrapper.treeBody.identityResource).toBe("managed/bravo_user");
  });

  it("refuses to emit a legacy wrapper", () => {
    expect(() =>
      emitWrapperJourney(
        caseWith({ given: { engine: "legacy" } }),
        SUBJECT,
        { runId: "leg" }
      )
    ).toThrow(/next-gen only/);
  });

  it("emits AM-lint-clean setup, instrumented subject and result scripts", async () => {
    const wrapper = emitWrapperJourney(
      caseWith({
        given: { sharedState: { username: "alice" }, transientState: { t: 1 } },
        expect: { outcome: "created" },
      }),
      SUBJECT,
      { runId: "lint", idFactory: sequentialIds() }
    );
    for (const script of wrapper.scripts) {
      const messages = await lintAmScript(
        script.source,
        `generated/aic-${script.role}.cjs`
      );
      expect(messages, script.name).toEqual([]);
    }
  });
});
