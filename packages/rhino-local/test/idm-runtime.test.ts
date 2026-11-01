/** Measured IDM behavior, exercised through the real compiled JVM lane. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { HostCallError, RhinoRunner, type HostCall, type JsonValue } from "../src/runner.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
}, 30_000);

async function value(source: string, bindings: Record<string, JsonValue> = {}): Promise<JsonValue> {
  const response = await runner.eval({
    engine: "idm",
    source,
    bindings,
    timeoutMs: 0,
  });
  expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
  return response.value;
}

describe("IDM engine configuration", () => {
  it.each([
    ["getLanguageVersion()", 200],
    ["getOptimizationLevel()", 9],
    ["getInstructionObserverThreshold()", 0],
    ["getMaximumInterpreterStackDepth()", 2147483647],
    ["getWrapFactory().isJavaPrimitiveWrap()", true],
    ["getFactory().getClass().getName()", "org.mozilla.javascript.ContextFactory"],
  ])("measures Context.%s", async (method, expected) => {
    expect(await value(`org.mozilla.javascript.Context.getCurrentContext().${method}`)).toBe(
      expected
    );
  });

  it("uses exactly the plain factory feature set (and rejects feature 22)", async () => {
    expect(
      await value(`var c = org.mozilla.javascript.Context.getCurrentContext();
      var yes=[]; for(var i=1;i<=21;i++) if(c.hasFeature(i)) yes.push(i); yes.join()`)
    ).toBe("3,5,6,14,16,20");
    expect(
      await value(
        "try {org.mozilla.javascript.Context.getCurrentContext().hasFeature(22)} catch(e) {String(e)}"
      )
    ).toContain("IllegalArgumentException: 22");
  });

  it("sets the locale per Context: IDM en and AM en-US even while jobs overlap", async () => {
    const [idm, am] = await Promise.all([
      runner.eval({
        engine: "idm",
        source: "String(org.mozilla.javascript.Context.getCurrentContext().getLocale())",
      }),
      runner.eval({
        source: "String(org.mozilla.javascript.Context.getCurrentContext().getLocale())",
      }),
    ]);
    expect(idm.value).toBe("en");
    expect(am.value).toBe("en_US");
    expect(await value("String(java.util.TimeZone.getDefault().getID())")).toBe("Etc/UTC");
  });

  it("ignores AM language/shutter options in the IDM engine", async () => {
    const response = await runner.eval({
      engine: "idm",
      languageVersion: 0,
      classAllowList: [],
      source: "let answer = 42; answer + java.lang.Integer.parseInt('1')",
    });
    expect(response).toMatchObject({ outcome: "ok", value: 43 });
  });

  it("compiled observer interrupts a runaway loop even inside try/catch and keeps serving", async () => {
    const response = await runner.eval({
      engine: "idm",
      timeoutMs: 100,
      source: "try {while(true){}} catch(e) {while(true){}}",
    });
    expect(response.outcome).toBe("timeout");
    expect(await value("6*7")).toBe(42);
  });

  it("AM's default and explicit engine produce the same response shape", async () => {
    const job = { id: "am-shape", source: "JSON.stringify({answer:42})" };
    const before = await runner.eval(job);
    expect(await runner.eval({ ...job, engine: "am" })).toEqual(before);
    expect(Object.keys(before)).toEqual(["id", "outcome", "valueKind", "value", "error"]);
  });
});

describe("IDM shell Global above deferred bindings", () => {
  it.each([
    "readFile",
    "runCommand",
    "spawn",
    "sync",
    "readUrl",
    "defineClass",
    "serialize",
    "quit",
    "load",
    "loadClass",
    "print",
    "version",
    "help",
    "gc",
    "seal",
    "toint32",
  ])("exposes shell %s", async (name) => {
    expect(await value(`typeof ${name}`)).toBe("function");
  });
  it("provides base64 and secureRandom; leaves sleep, json and util undefined", async () => {
    expect(
      await value(
        "[btoa('hi'),atob('aGk='),typeof secureRandom,typeof sleep,typeof json,typeof util,typeof org].join()"
      )
    ).toBe("aGk=,hi,object,undefined,undefined,undefined,object");
  });
  it("uses the measured prototype direction and resolves named deferred bindings", async () => {
    expect(
      await value(
        "Object.prototype.toString.call(this)+';'+Object.prototype.toString.call(Object.getPrototypeOf(this))+';'+input.a",
        { input: { a: 42 } }
      )
    ).toBe("[object global];[object ScriptableWithDeferredBinding];42");
  });
  // Discriminates reversing the prototype chain, or enumerating a NativeObject binding table.
  it("for (p in this) inside try/catch still fails the job", async () => {
    const response = await runner.eval({
      engine: "idm",
      source: "try { for(var p in this) {} } catch(e) { 'caught' }",
    });
    expect(response.outcome).toBe("runtime_error");
    expect(response.error?.class).toBe("java.lang.UnsupportedOperationException");
    expect(response.error?.message).toContain("There is no way know all the bound ids");
  });
  it("isolates Global and standard-object mutations across jobs", async () => {
    await value("Object.prototype.polluted=1; var leaked=2;");
    expect(await value("typeof ({}).polluted+';'+typeof leaked")).toBe("undefined;undefined");
  });
});

describe("IDM ScriptableMap replicas", () => {
  const r = { k: "text", objects: [{ a: 1 }, { a: 2 }] };
  it.each([
    ["Object.prototype.toString.call(r)", "[object ScriptableMap]"],
    [
      "Object.getPrototypeOf(r)===Object.prototype && r instanceof Object && r.constructor===Object",
      true,
    ],
    ["Object.keys(r).join()", "k,objects"],
    ["var keys=[]; for(var k in r) keys.push(k); keys.join()", "k,objects"],
    ["'k' in r && r.hasOwnProperty('k')", true],
    ["Object.getOwnPropertyDescriptor(r,'k')===undefined", true],
    ["delete r.k; 'k' in r", false],
    ["r.o={b:1,a:2,c:3,zz:4,aa:5}; Object.keys(r.o).join()", "zz,aa,a,b,c"],
    ["r.o={a:1}; r.o===r.o", true],
    ["r.x=Infinity; r.x===9223372036854775807", true],
    ["r.y=-0; 1/r.y===Infinity", true],
    ["r.w=NaN; String(r.w)", "NaN"],
    ["typeof r.k", "string"],
  ])("%s", async (source, expected) => {
    expect(await value(source, { r })).toBe(expected);
  });
  // NativeObject has own names and descriptors; generic Java wrappers fail Object.keys.
  it("Object.getOwnPropertyNames(r) is empty while Object.keys(r) is not", async () => {
    expect(
      await value("Object.getOwnPropertyNames(r).length+';'+Object.keys(r).length", { r })
    ).toBe("0;2");
  });
  it("String(r) is pretty JSON text", async () => {
    const text = await value("String(r)", { r });
    expect(typeof text).toBe("string");
    expect(String(text)).toContain("\n");
    expect(JSON.parse(String(text))).toEqual(r);
  });
  it("converts nested assigned values to Java maps and returns binding mutations", async () => {
    const response = await runner.eval({
      engine: "idm",
      bindings: { object: {} },
      source:
        "object.o={b:1,a:2}; object.when=new Date(0); object.nan=NaN; object.inf=Infinity; object",
    });
    expect(response.outcome).toBe("ok");
    expect(response.value).toEqual({
      o: { a: 2, b: 1 },
      when: {},
      nan: "NaN",
      inf: Number("9223372036854775807"),
    });
    expect(response.bindings?.object).toEqual(response.value);
  });
  it("all object-valued bindings and nested bindings are replicas, scalars are primitives", async () => {
    expect(
      await value(
        "[object,oldObject,newObject,request,context,value,property,input].map(function(r){return Object.prototype.toString.call(r)}).join()",
        Object.fromEntries(
          [
            "object",
            "oldObject",
            "newObject",
            "request",
            "context",
            "value",
            "property",
            "input",
          ].map((k) => [k, { a: {} }])
        )
      )
    ).toBe(Array(8).fill("[object ScriptableMap]").join());
    expect(
      await value(
        "[typeof input.s,typeof input.n,typeof input.b,Object.prototype.toString.call(input.o)].join()",
        { input: { s: "x", n: 1, b: true, o: {} } }
      )
    ).toBe("string,number,boolean,[object ScriptableMap]");
  });
  it("resourceName bindings are JavaObjects with a path string", async () => {
    const response = await runner.eval({
      engine: "idm",
      bindings: { resourceName: "managed/example/1", input: "ordinary" },
      resourceNameBindings: ["resourceName"],
      source:
        "[typeof resourceName,Object.prototype.toString.call(resourceName),String(resourceName),typeof input].join()",
    });
    expect(response).toMatchObject({
      outcome: "ok",
      value: "object,[object JavaObject],managed/example/1,string",
    });
  });
});

describe("IDM list replicas", () => {
  const bindings = { q: { result: [{ a: 1 }, { a: 2 }] } };
  it.each([
    ["Array.isArray(q.result)", true],
    ["Object.prototype.toString.call(q.result)", "[object Array]"],
    ["Object.keys(q.result).join()", "0,1"],
    ["q.result.map(function(r){return r.a}).join()", "1,2"],
    ["q.result.filter(function(r){return r.a>1}).length", 1],
    ["q.result.indexOf(q.result[1])", 1],
    ["var sum=0; q.result.forEach(function(r){sum+=r.a}); sum", 3],
    [
      "q.t=[1,null,3]; q.t.length+';'+(1 in q.t)+';'+q.t.map(function(v){return 'v'+v}).join()",
      "3;false;v1,vnull,v3",
    ],
  ])("%s", async (source, expected) => {
    expect(await value(source, bindings)).toBe(expected);
  });
  // A NativeArray passes Array.isArray but produces [...] here.
  it('JSON.stringify(list) gives {"0":…}, while a member of a stringified map is an array', async () => {
    expect(await value("JSON.stringify(q.result)", bindings)).toBe('{"0":{"a":1},"1":{"a":2}}');
    expect(await value("JSON.stringify(q)", bindings)).toBe('{"result":[{"a":1},{"a":2}]}');
  });
  it("String(list) is pretty JSON array text", async () => {
    const text = String(await value("String(q.result)", bindings));
    expect(text).toContain("\n");
    expect(JSON.parse(text)).toEqual(bindings.q.result);
  });
});

function failure(
  exceptionClass: string,
  message: string,
  code = 400,
  detail: JsonValue = {}
): never {
  throw new HostCallError({ class: exceptionClass, message, code, detail });
}
const crest = "org.forgerock.json.resource.";

describe("IDM Java host surfaces", () => {
  it.each([
    ["openidm.read(5)", "read(Integer)"],
    ["openidm.delete('managed/example/1')", "delete(String)"],
    ["openidm.action('managed/example','go',{},null)", "action(String,String,HashMap,null)"],
    ["logger.info({a:1})", "info(HashMap)"],
    ["identityServer.getProperty('missing',1,'yes')", "getProperty(String,Integer,String)"],
  ])("rejects Java overload %s before routing", async (source, signature) => {
    let calls = 0;
    const response = await runner.eval(
      {
        engine: "idm",
        source: `try {${source}} catch(e) {e.name+';'+e.message}`,
      },
      {
        onCall: () => {
          calls++;
          return null;
        },
      }
    );
    expect(response).toMatchObject({
      outcome: "ok",
      value: `JavaException;java.lang.NoSuchMethodException: Method not found ${signature}`,
    });
    expect(calls).toBe(0);
  });
  it.each(["read", "create", "update", "patch", "delete", "query", "action"] as const)(
    "routes openidm.%s through the real JVM and wraps the reply",
    async (method) => {
      const expressions = {
        read: "'managed/example/1'",
        create: "'managed/example','1',{a:1}",
        update: "'managed/example/1',null,{a:1}",
        patch: "'managed/example/1',null,[{operation:'replace',field:'/a',value:1}]",
        delete: "'managed/example/1',null",
        query: "'managed/example',{_queryFilter:'true'}",
        action: "'endpoint/example','go',{a:1}",
      };
      const calls: HostCall[] = [];
      const response = await runner.eval(
        {
          engine: "idm",
          source: `var r=openidm.${method}(${expressions[method]}); Object.prototype.toString.call(r)+';'+r.a`,
        },
        {
          onCall: (call) => {
            calls.push(call);
            return { a: 42 };
          },
        }
      );
      expect(response).toMatchObject({
        outcome: "ok",
        value: "[object ScriptableMap];42",
      });
      expect(calls).toHaveLength(1);
      expect(calls[0]).toMatchObject({
        type: "call",
        id: response.id,
        seq: 1,
        method,
      });
    }
  );
  it.each(["managed/example/missing", "unrouted/path"])(
    "passes a missing read to the host and delivers null: %s",
    async (resource) => {
      const response = await runner.eval(
        { engine: "idm", source: `openidm.read(${JSON.stringify(resource)})` },
        { onCall: () => null }
      );
      expect(response).toMatchObject({
        outcome: "ok",
        valueKind: "null",
        value: null,
      });
    }
  );
  it.each([
    ["openidm.delete('managed/example/missing',null)", "NotFoundException", "missing", 404],
    [
      "openidm.action('managed/example','unknown',{})",
      "BadRequestException",
      "Expecting String containing one of: patch triggerSyncCheck updateLastSync",
      400,
    ],
    [
      "openidm.query('managed/example',{})",
      "BadRequestException",
      "Only one of [_queryId, _queryExpression, _queryFilter] is supported; multiple detected",
      400,
    ],
    [
      "openidm.query('repo/example',{_queryExpression:'select'})",
      "NotSupportedException",
      "Query Expressions are not supported when using DS as the repo",
      405,
    ],
    [
      "openidm.query('managed/example',{_queryFilter:'true'},['_id'],function(){})",
      "BadRequestException",
      "The value '[_id]' for additional parameter '_fields' is not of expected type String",
      400,
    ],
  ])("delivers routed CREST errors for %s", async (source, klass, message, code) => {
    // Query/filter/store policy is supplied by the host; the JVM preserves CREST failure semantics.
    const response = await runner.eval(
      {
        engine: "idm",
        source: `try {${source}} catch(e) {e.name+';'+e.message+';'+e.javaException.getCode()}`,
      },
      {
        onCall: () => failure(crest + klass, message, code),
      }
    );
    expect(response).toMatchObject({
      outcome: "ok",
      value: `JavaException;${crest}${klass}: ${message};${code}`,
    });
  });
  it("JavaException exposes only fileName,lineNumber as keys, and getCode/getDetail", async () => {
    const response = await runner.eval(
      {
        engine: "idm",
        source: `try {openidm.read('x')} catch(e) {
      [e.name,e.message,Object.keys(e).join(),e.javaException.getCode(),JSON.stringify(e.javaException.getDetail())].join(';')}`,
      },
      {
        onCall: () =>
          failure(crest + "ForbiddenException", "denied", 403, {
            failedPolicyRequirements: [{ name: "example" }],
          }),
      }
    );
    expect(response).toMatchObject({
      outcome: "ok",
      value: `JavaException;${crest}ForbiddenException: denied;fileName,lineNumber;403;{"failedPolicyRequirements":[{"name":"example"}]}`,
    });
  });
  it("query callback runs once per wrapped row and removes result/resultCount", async () => {
    const response = await runner.eval(
      {
        engine: "idm",
        source: `var seen=[];
      var q=openidm.query('managed/example',{_queryFilter:'true'},function(row){seen.push(Object.prototype.toString.call(row)+row.a)});
      seen.join()+';'+('result' in q)+';'+('resultCount' in q)+';'+q.remainingPagedResults`,
      },
      {
        onCall: () => ({
          result: [{ a: 1 }, { a: 2 }],
          resultCount: 2,
          remainingPagedResults: -1,
        }),
      }
    );
    expect(response).toMatchObject({
      outcome: "ok",
      value: "[object ScriptableMap]1,[object ScriptableMap]2;false;false;-1",
    });
  });
  it("drops a non-final function argument and preserves _fields inside params", async () => {
    const calls: HostCall[] = [];
    const response = await runner.eval(
      {
        engine: "idm",
        source: `openidm.query('managed/example',openidm.read,{_queryFilter:'true',_fields:['_id']})`,
      },
      {
        onCall: (call) => {
          calls.push(call);
          return { result: [] };
        },
      }
    );
    expect(response.outcome).toBe("ok");
    expect(calls[0]?.args).toEqual(["managed/example", { _queryFilter: "true", _fields: ["_id"] }]);
  });
  it("String(openidm.read) throws InternalError for ScriptableFunction", async () => {
    expect(await value("try{String(openidm.read)}catch(e){e.name+';'+e.message}")).toContain(
      "InternalError;Primitive type expected (had ScriptableFunction instead)"
    );
  });
  it.each(["parseFilter", "hash", "encrypt", "isHashed", "matches", "decrypt", "isEncrypted"])(
    "openidm.%s fails as missing",
    async (method) => {
      const response = await runner.eval({
        engine: "idm",
        source: `openidm.${method}('x')`,
      });
      expect(response.outcome).toBe("runtime_error");
      expect(response.error?.message).toContain(
        `rhino-local: openidm.${method} is not emulated yet`
      );
    }
  );
  it("require fails explicitly as missing", async () => {
    const response = await runner.eval({
      engine: "idm",
      source: "require('lib/lodash')",
    });
    expect(response.outcome).toBe("runtime_error");
    expect(response.error?.message).toContain("rhino-local: require is not emulated yet");
  });
  it("collects logger placeholders and console output in order without host calls", async () => {
    let calls = 0;
    const response = await runner.eval(
      {
        engine: "idm",
        source:
          "logger.info('v={}',{a:1}); console.log('second',2); logger.warn('third'); Object.keys(console).join()",
      },
      {
        onCall: () => {
          calls++;
          return null;
        },
      }
    );
    expect(response).toMatchObject({ outcome: "ok", value: "log" });
    expect(response.logs).toEqual([
      {
        surface: "logger",
        level: "info",
        message: "v={a=1}",
        args: ["v={}", { a: 1 }],
      },
      {
        surface: "console",
        level: "log",
        message: "second 2",
        args: ["second", 2],
      },
      { surface: "logger", level: "warn", message: "third", args: ["third"] },
    ]);
    expect(calls).toBe(0);
  });
  it.each([null, "fallback", 3, true, { a: 1 }, [1, 2]])(
    "identityServer returns any default as an IDM value: %j",
    async (fallback) => {
      expect(
        await value("identityServer.getProperty('missing',input)", {
          input: fallback,
        })
      ).toEqual(fallback);
    }
  );
});

describe("IDM throw classification for emulator HTTP policy", () => {
  it.each([
    [
      "throw {code:418,message:'tea',detail:{a:1}}",
      { kind: "object", code: 418, message: "tea", detail: { a: 1 } },
    ],
    ["throw {code:400}", { kind: "object", code: 400, message: "[object Object]" }],
    ["throw {code:'404'}", { kind: "object", code: "404" }],
    ["throw new Error('plain error')", { kind: "error", message: "Error: plain error" }],
    ["throw 'bad pval'", { kind: "string", value: "bad pval", message: "bad pval" }],
    ["throw 12", { kind: "primitive", value: 12 }],
    ["throw null", { kind: "primitive", value: null }],
    ["missingReference", { kind: "error", class: "ReferenceError" }],
  ])("classifies %s", async (source, thrown) => {
    const response = await runner.eval({ engine: "idm", source });
    expect(response.outcome).toBe("runtime_error");
    expect(response.thrown).toMatchObject(thrown);
  });
  it("preserves host class, message, code and detail for an uncaught JavaException", async () => {
    const response = await runner.eval(
      { engine: "idm", source: "openidm.read('x')" },
      {
        onCall: () => failure(crest + "ForbiddenException", "denied", 403, { a: 1 }),
      }
    );
    expect(response.thrown).toEqual({
      kind: "javaException",
      class: crest + "ForbiddenException",
      message: "denied",
      code: 403,
      detail: { a: 1 },
    });
  });
  it("keeps author source locations and distinguishes compile errors", async () => {
    const response = await runner.eval({
      engine: "idm",
      source: "var good=1;\nlet = ;",
      sourceName: "idm-author.js",
    });
    expect(response.outcome).toBe("compile_error");
    expect(response.error).toMatchObject({
      sourceName: "idm-author.js",
      line: 2,
    });
  });
});
