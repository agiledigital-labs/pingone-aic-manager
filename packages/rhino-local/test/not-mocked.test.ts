import { describe, expect, it } from "vitest";
import { asRecord, loadGenerated } from "./load-generated.ts";

describe("unimplemented mocks throw", () => {
  const sandbox = loadGenerated();

  it("reports binding, method and arity", () => {
    const logger = asRecord(sandbox.logger, "logger");
    const info = logger.info as (message: string) => void;
    expect(() => info("hello")).toThrow(
      /^rhino-local: not mocked: logger\.info arity=1 overload=\[info\(msg: string\)\]$/
    );
  });

  it("lists every same-arity overload when types do not unique the call", () => {
    const logger = asRecord(sandbox.logger, "logger");
    const info = logger.info as (message: string, extra: object) => void;
    expect(() => info("hello", { x: 1 })).toThrow(
      /logger\.info arity=2 overload=\[info\(format: string, arg: object\) \| info\(msg: string, t: object\)\]/
    );
  });

  it("selects the array overload by argument kind", () => {
    const logger = asRecord(sandbox.logger, "logger");
    const info = logger.info as (message: string, extra: unknown[]) => void;
    expect(() => info("hello", ["a"])).toThrow(
      /logger\.info arity=2 overload=\[info\(format: string, arguments: array\)\]/
    );
  });

  it("uses the nested field path as the binding name", () => {
    const subtle = asRecord(
      asRecord(asRecord(sandbox.utils, "utils").crypto, "utils.crypto").subtle,
      "utils.crypto.subtle"
    );
    const sign = subtle.sign as (algorithm: string, key: unknown[], data: unknown[]) => void;
    expect(() => sign("HS256", [], [])).toThrow(
      /rhino-local: not mocked: utils\.crypto\.subtle\.sign arity=3 overload=\[sign\(algorithm: string, key: array, data: array\)\]/
    );
  });
});
