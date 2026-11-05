import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { loadBehaviour, sandboxFaults } from "./load-behaviour.ts";

// E19 decision B: a wrong argument count is AIC's own catchable refusal where
// the accepted range has a basis (measured, or AIC's binding introspection),
// and a harness fault where the range is the harness's guess.

const repoRoot = fileURLToPath(new URL("../../../../", import.meta.url));
const runtimeSource = readFileSync(
  fileURLToPath(new URL("../../src/bindings/rhino/runtime.cjs", import.meta.url)),
  "utf8"
);

interface Element {
  elementType: string;
  name: string;
  parameters?: unknown[];
  elements?: Element[];
}
/** binding.method -> accepted argument counts, from AIC's introspection. */
function introspectedCounts(): Map<string, Set<number>> {
  const doc = JSON.parse(
    readFileSync(`${repoRoot}docs/api/bindings/scripted-decision-next.json`, "utf8")
  ) as { bindings: Element[] };
  const out = new Map<string, Set<number>>();
  const walk = (node: Element, prefix: string): void => {
    for (const element of node.elements ?? []) {
      if (element.elementType === "method") {
        const key = `${prefix}.${element.name}`;
        const counts = out.get(key) ?? new Set<number>();
        counts.add(element.parameters?.length ?? 0);
        out.set(key, counts);
      } else {
        walk(element, `${prefix}.${element.name}`);
      }
    }
  };
  for (const binding of doc.bindings) {
    walk(binding, binding.name);
  }
  return out;
}

type Basis = { java: string; provenance: "measured" | "assumed" };

const given = {
  existingSession: { UserId: "alice" },
  bindings: { cacheManager: {}, journey: {} },
};

function resolve(sandbox: Record<string, unknown>, path: string): ((...args: unknown[]) => unknown) | undefined {
  let target: unknown = sandbox;
  for (const part of path.split(".")) {
    if (target === null || target === undefined) {
      return undefined;
    }
    target = (target as Record<string, unknown>)[part];
  }
  return typeof target === "function" ? (target as (...args: unknown[]) => unknown) : undefined;
}

// Strings fit every overload but one: confirmationCallback's three-argument
// forms start with an int, and are chosen by type as Rhino would.
function argsFor(path: string, count: number): unknown[] {
  if (path === "callbacksBuilder.confirmationCallback" && count === 3) {
    return [0, "x", "x"];
  }
  return Array.from({ length: count }, () => "x");
}

function callWith(fn: (...args: unknown[]) => unknown, args: unknown[]): unknown {
  try {
    fn(...args);
    return undefined;
  } catch (error) {
    return error;
  }
}

describe("arity provenance", () => {
  const basis = loadBehaviour(given).__rhinoLocalArityBasis as Record<string, Basis>;
  const introspected = introspectedCounts();

  it("names exactly the two measured methods", () => {
    expect(
      Object.entries(basis)
        .filter(([, b]) => b.provenance === "measured")
        .map(([path]) => path)
        .sort()
    ).toEqual(["existingSession.get", "utils.crypto.randomUUID"]);
  });

  it("takes every assumed range and class from AIC's introspection", () => {
    for (const [path, b] of Object.entries(basis)) {
      if (b.provenance === "assumed") {
        expect(introspected.has(path), path).toBe(true);
      }
    }
  });

  // The discriminating rows: a count AIC accepts must not be refused, and a
  // count it refuses must be refused catchably, in AIC's words, with no fault.
  it("refuses exactly the counts AIC's overload set has no method for", () => {
    // utils is overlaid only on the JVM, whose host ops it needs (node:vm has
    // the generated not-mocked stub); corpus binding-utils-interop compares
    // randomUUID(number) with AIC's live text there.
    const jvmOnly = ["utils.crypto.getRandomValues", "utils.crypto.randomUUID"];
    for (const [path, b] of Object.entries(basis)) {
      if (jvmOnly.includes(path)) {
        continue;
      }
      const sandbox = loadBehaviour(given);
      const fn = resolve(sandbox, path)!;
      expect(fn, path).toBeTypeOf("function");
      const accepted = introspected.get(path);
      const method = path.substring(path.lastIndexOf(".") + 1);
      for (let count = 0; count <= 12; count += 1) {
        const ok = path.startsWith("logger.")
          ? count >= 1
          : path === "existingSession.get"
            ? count === 1
            : accepted!.has(count);
        const error = callWith(fn, argsFor(path, count)) as { name?: string; message?: string } | undefined;
        const refused = typeof error?.message === "string" && error.message.startsWith("Can't find method");
        expect(refused, `${path} with ${count}`).toBe(!ok);
        if (!ok) {
          expect(error!.name).toBe("InternalError");
          expect(error!.message).toBe(
            `Can't find method ${b.java}.${method}(${Array(count).fill("string").join(",")}).`
          );
        }
      }
      expect(
        sandboxFaults(sandbox).filter((f) => f.id.startsWith("arity")),
        path
      ).toEqual([]);
    }
  });

  it("names a refused argument the way Rhino's scriptSignature does", () => {
    const sandbox = loadBehaviour(given);
    const fn = resolve(sandbox, "nodeState.get")!;
    const error = callWith(() => fn(null, 1, true, undefined, [], {}, () => 0), []) as Error;
    expect(error.message).toBe(
      "Can't find method org.forgerock.openam.auth.node.api.NodeStateScriptWrapper.get(null,number,boolean,undefined,object,object,function)."
    );
  });

  it("faults a guessed range under its own id, never claiming AIC's behaviour", () => {
    const sandbox = loadBehaviour({ ...given, requestHeaders: { a: ["b"] } });
    const error = callWith(resolve(sandbox, "requestHeaders.get")!, []) as Error;
    expect(error.message).toBe(
      "rhino-local: map.get arity=0 (the harness accepts 1; AIC's accepted argument counts here are unmeasured)"
    );
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["arity-unknown/map.get"]);
  });

  it("has no basis on the legacy engine, which has no introspection", () => {
    const sandbox = loadBehaviour({ ...given, engine: "legacy" });
    callWith(resolve(sandbox, "nodeState.get")!, []);
    callWith(resolve(sandbox, "existingSession.get")!, []);
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual([
      "arity-unknown/nodeState.get",
      "arity-unknown/existingSession.get",
    ]);
  });

  // Every literal path an arity check names is either in the basis table or
  // one of these, each a range with no measurement and no introspection row.
  // The computed ones ("logger." + level, the attribute-input callbacks) are
  // covered by the introspection test above, which reads every table row.
  it("tags every arity site", () => {
    const guessed = [
      "map.get",
      "map.containsKey",
      "existingSession.put",
      "existingSession.containsKey",
      "idRepository.getIdentity().setAttribute",
      "idRepository.getIdentity().addAttribute",
      "idRepository.getIdentity().store",
      "secret.getAsUtf8",
      "require",
    ];
    const literal = [
      ...runtimeSource.matchAll(/__rhinoLocal(?:ExpectArity|ArityRefusal)\(\s*"([^"]+)"\s*,/g),
    ].map((m) => m[1]!);
    const untagged = [...new Set(literal)].filter(
      (path) => !(path in basis) && !guessed.includes(path)
    );
    expect(untagged).toEqual([]);
    for (const path of guessed) {
      expect(path in basis, path).toBe(false);
    }
  });
});
