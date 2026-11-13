import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { loadBehaviour, runScriptWithFaults, sandboxFaults } from "./load-behaviour.ts";

// E19 decision B: a wrong argument count is AIC's own catchable refusal where
// the accepted range has a basis (measured, or AIC's binding introspection),
// and a harness fault where the range is the harness's guess.

const repoRoot = fileURLToPath(new URL("../../../../", import.meta.url));
const runtimeSource = readFileSync(
  fileURLToPath(new URL("../../src/bindings/rhino/runtime.cjs", import.meta.url)),
  "utf8"
);

interface Element {
  elementType?: string;
  name: string;
  javaClass?: string;
  parameters?: Array<{ javaScriptType?: string }>;
  elements?: Element[];
}
/**
 * binding.method -> accepted argument counts, declaring class, and the lowest
 * count a varargs reading of a trailing array parameter would accept, from
 * AIC's introspection.
 */
interface Overloads {
  counts: Set<number>;
  javaClass: string | undefined;
  arrayTail: number | undefined;
}
function introspected(): Map<string, Overloads> {
  const doc = JSON.parse(
    readFileSync(`${repoRoot}docs/api/bindings/scripted-decision-next.json`, "utf8")
  ) as { bindings: Element[] };
  const out = new Map<string, Overloads>();
  const walk = (node: Element, prefix: string): void => {
    for (const element of node.elements ?? []) {
      if (element.elementType === "method") {
        const key = `${prefix}.${element.name}`;
        const entry = out.get(key) ?? { counts: new Set<number>(), javaClass: node.javaClass, arrayTail: undefined };
        const parameters = element.parameters ?? [];
        entry.counts.add(parameters.length);
        if (parameters.at(-1)?.javaScriptType === "array") {
          entry.arrayTail = Math.min(entry.arrayTail ?? Infinity, parameters.length - 1);
        }
        out.set(key, entry);
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

type Basis = { java: string; provenance: "measured" | "assumed"; arrayTail?: number; varargs?: number };

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
  // A merge takes a map; a string is refused by type, not count (2026-11-06).
  if (path === "nodeState.mergeShared" || path === "nodeState.mergeTransient") {
    return Array.from({ length: count }, () => ({}));
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
  const table = introspected();

  it("names exactly the measured methods", () => {
    expect(
      Object.entries(basis)
        .filter(([, b]) => b.provenance === "measured")
        .map(([path]) => path)
        .sort()
    ).toEqual([
      "existingSession.get", "logger.debug", "logger.error", "logger.info", "logger.warn",
      "nodeState.mergeShared", "utils.crypto.randomUUID",
    ]);
    // E19 live probe 2026-11-06: the four probed levels are true varargs; trace was not probed.
    expect(Object.entries(basis).filter(([, b]) => b.varargs !== undefined).map(([path]) => path).sort())
      .toEqual(["logger.debug", "logger.error", "logger.info", "logger.warn"]);
  });

  it("takes every assumed range and class from AIC's introspection", () => {
    for (const [path, b] of Object.entries(basis)) {
      expect(["measured", "assumed"], path).toContain(b.provenance);
      if (b.provenance === "assumed") {
        expect(table.has(path), path).toBe(true);
      }
      // A measured method AIC's introspection also lists must agree with it;
      // existingSession's class is known only from the measured refusal.
      if (table.has(path)) {
        expect(b.java, path).toBe(table.get(path)!.javaClass);
      }
      // A trailing array parameter may be Java varargs; the count from which
      // that reading decides must come from the introspection too.
      if (b.provenance === "assumed") {
        expect(b.arrayTail, path).toBe(table.get(path)!.arrayTail);
      }
    }
  });

  // The discriminating rows: a count AIC accepts must not be refused; a count
  // no overload takes must be refused catchably, in AIC's words, with no
  // fault -- unless only a varargs reading of a trailing array parameter
  // (P3-1) decides it, which is the arity fault: logger with 4 arguments was
  // accepted, and openidm.read with 4 refused catchably, on that reading.
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
      const accepted = table.get(path)?.counts;
      const method = path.substring(path.lastIndexOf(".") + 1);
      const faulted: number[] = [];
      for (let count = 0; count <= 12; count += 1) {
        const ok = path === "existingSession.get" ? count === 1
          : accepted!.has(count) || (b.varargs !== undefined && count >= b.varargs);
        const unknown = !ok && b.arrayTail !== undefined && count >= b.arrayTail;
        const before = sandboxFaults(sandbox).length;
        const error = callWith(fn, argsFor(path, count)) as { name?: string; message?: string } | undefined;
        const refused = typeof error?.message === "string" && error.message.startsWith("Can't find method");
        expect(refused, `${path} with ${count}`).toBe(!ok && !unknown);
        if (unknown) {
          expect(sandboxFaults(sandbox).slice(before).map((f) => f.id), `${path} with ${count}`).toEqual([
            `arity-unknown/${path}`,
          ]);
          faulted.push(count);
        }
        if (!ok && !unknown) {
          expect(error!.name).toBe("InternalError");
          expect(error!.message).toBe(
            `Can't find method ${b.java}.${method}(${argsFor(path, count).map((a) => typeof a === "string" ? "string" : "object").join(",")}).`
          );
        }
      }
      expect(
        sandboxFaults(sandbox).filter((f) => f.id.startsWith("arity")).length,
        path
      ).toBe(faulted.length);
    }
  });

  it("faults the reviewer's varargs-reading repros", () => {
    const sandbox = loadBehaviour(given);
    const logger = sandbox.logger as Record<string, (...args: unknown[]) => unknown>;
    const openidm = sandbox.openidm as Record<string, (...args: unknown[]) => unknown>;
    // trace was not probed on 2026-11-06, so its varargs reading is still the fault.
    expect((callWith(logger.trace!, ["{} {} {}", 1, 2, 3]) as Error).message).toMatch(/^rhino-local: logger.trace arity=4 /);
    expect((callWith(openidm.read!, ["managed/user/x", null, ["_id"], "x"]) as Error).message).toMatch(
      /^rhino-local: openidm.read arity=4 .*varargs.*unmeasured/
    );
    expect(sandboxFaults(sandbox).map((f) => f.id)).toEqual(["arity-unknown/logger.trace", "arity-unknown/openidm.read"]);
    // The control: below the array tail the overload set still decides.
    const control = loadBehaviour(given);
    const read = (control.openidm as Record<string, (...args: unknown[]) => unknown>).read!;
    expect((callWith(read, []) as Error).message).toBe(
      "Can't find method org.forgerock.openam.scripting.wrappers.IdmIntegrationServiceScriptWrapper.read()."
    );
    expect(callWith((control.logger as Record<string, (...args: unknown[]) => unknown>).info!, ["{} {}", 1, 2])).toBeUndefined();
    expect(sandboxFaults(control)).toEqual([]);
  });

  // The 2026-11-06 probe's own calls (probes p1-logger-arity), and the lines am-core logged.
  it("accepts any count on the four measured levels and logs them as AIC did", () => {
    const script = [
      'logger.info("ZZEMU-L T4 info {} {} {}", "A", "B", "C");',
      'logger.error("ZZEMU-L T6 error {} {} {} {} {}", "A", "B", "C", "D", "E");',
      'logger.warn("ZZEMU-L T5 warn {} {} {} {}", "A", "B", "C", "D");',
      'logger.debug("ZZEMU-L T6 debug {} {} {} {} {}", "A", "B", "C", "D", "E");',
      'logger.info("ZZEMU-L C6 info array {} {} {}", ["A", "B", "C"]);',
      'logger.info("ZZEMU-L T5m info {} {} {} {}", 1, true, null, {k: "v"});',
      'logger.info("n {} {} {} {}", 2.5, -0, 1e7, 0.0001);',
      'var w; try { logger.warning("x {} {} {} {}", "A", "B", "C", "D"); w = "ok"; } catch (e) { w = e.name; }',
      'logger.info("warning {}", w);',
    ].join("\n");
    const { effects, faults } = runScriptWithFaults(script, given);
    expect(faults).toEqual([]);
    expect(effects.logs).toEqual([
      { level: "info", message: "ZZEMU-L T4 info A B C" },
      { level: "error", message: "ZZEMU-L T6 error A B C D E" },
      { level: "warn", message: "ZZEMU-L T5 warn A B C D" },
      { level: "debug", message: "ZZEMU-L T6 debug A B C D E" },
      { level: "info", message: "ZZEMU-L C6 info array A B C" },
      { level: "info", message: "ZZEMU-L T5m info 1.0 true null [object Object]" },
      // Java's Double.toString for the rest: not in the probe, but what a Double prints.
      { level: "info", message: "n 2.5 -0.0 1.0E7 1.0E-4" },
      { level: "info", message: "warning TypeError" },
    ]);
  });

  it("names a refused argument the way Rhino's scriptSignature does", () => {
    const sandbox = loadBehaviour(given);
    const fn = resolve(sandbox, "nodeState.get")!;
    const error = callWith(() => fn(null, 1, true, undefined, [], {}, () => 0), []) as Error;
    expect(error.message).toBe(
      "Can't find method org.forgerock.openam.auth.node.api.NodeStateScriptWrapper.get(null,number,boolean,org.mozilla.javascript.Undefined,object,object,function)."
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
