// Next-gen scripted decision that drives rhino-lib-common-scope-probe. Run it
// twice with different `run` query parameters: the "before" fields of run 2 say
// whether anything written onto the top-level scope in run 1 survived into a
// later journey (the leak case option A must rule out). Seeds nodeState itself,
// so the journey needs no earlier nodes. Expects header x-aic-probe, query
// parameters probeq and run, and cookie aicprobe. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = "ok";
}

function attempt(fn) {
  try {
    return { ok: true, value: fn() };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
}

var self = this;
var p = { feature: "lib-common-scope" };

try {
  p.run = String(requestParameters.get("run")[0]);

  // What is top-level `this`?
  p.typeofThis = typeof self;
  p.thisIsGlobalThis = attempt(function () {
    return typeof globalThis === "undefined" ? "no-globalThis" : self === globalThis;
  });
  p.thisNodeStateSame = attempt(function () {
    return self.nodeState === nodeState;
  });
  p.thisActionSame = attempt(function () {
    return self.action === action;
  });
  p.inOperator = attempt(function () {
    return {
      nodeState: "nodeState" in self,
      realm: "realm" in self,
      logger: "logger" in self,
      myValue1: "myValue1" in self,
    };
  });
  p.arrowThisSame = attempt(function () {
    var f = () => this;
    return f() === self;
  });

  // Leak check: anything left over from an earlier run, before we write.
  p.before = {
    bareMyValue1: typeof myValue1 === "undefined" ? null : String(myValue1),
    bareConfig: typeof config === "undefined" ? null : String(config),
    scopeCounter: attempt(function () {
      return self.aicProbeScopeCounter === undefined ? null : self.aicProbeScopeCounter;
    }),
  };
  p.scopeCounterAfter = attempt(function () {
    self.aicProbeScopeCounter = (self.aicProbeScopeCounter || 0) + 1;
    return self.aicProbeScopeCounter;
  });

  nodeState.putShared("myValue1", "hello-" + p.run);
  nodeState.putShared("myValue2", { region: "au", run: p.run });

  var lib = require("rhino-lib-common-scope-probe");
  p.moduleCalls = lib.hit();
  p.bindingsViaThis = lib.readBindings(self);

  // Option A: an arrow defined BEFORE injection, called after.
  var early = () => ({
    myValue1: typeof myValue1 === "undefined" ? null : String(myValue1),
    config: typeof config === "undefined" ? null : JSON.stringify(config),
  });
  p.inject = lib.inject(self, {
    myValue1: "injected-" + p.run,
    config: { region: "au", run: p.run },
  });
  p.optionA = {
    earlyArrow: attempt(early),
    lateArrow: attempt(() => (typeof myValue1 === "undefined" ? null : String(myValue1))),
    // inject() reported scope[k] !== values[k] for a concatenated string; is
    // the bare read still a JS string, or did the scope convert it?
    bareTypes: attempt(() => ({
      myValue1: typeof myValue1,
      myValue1Tag: Object.prototype.toString.call(myValue1),
      myValue1Eq: myValue1 === "injected-" + p.run,
      myValue1LooseEq: myValue1 == "injected-" + p.run,
      config: typeof config,
      configRegionType: typeof config.region,
    })),
    namedFunction: attempt(function () {
      return typeof config === "undefined" ? null : config.region;
    }),
    viaRun: attempt(() =>
      lib.run(self, { myValue1: "viarun-" + p.run }, () => String(myValue1))
    ),
  };

  // Which primitive types survive a write onto the scope? A java.lang.Boolean
  // false is truthy in JS, so this decides whether option A is safe at all.
  p.primitives = attempt(() => {
    lib.inject(self, {
      aicLit: "literal",
      aicNum: 42,
      aicFalse: false,
      aicZero: 0,
      aicNull: null,
      aicArr: ["a", "b"],
    });
    return {
      lit: [typeof aicLit, aicLit === "literal"],
      num: [typeof aicNum, aicNum === 42],
      falseTruthy: [typeof aicFalse, aicFalse ? "truthy" : "falsy"],
      zeroTruthy: [typeof aicZero, aicZero ? "truthy" : "falsy"],
      nul: [typeof aicNull, aicNull === null],
      arr: [typeof aicArr, Array.isArray(aicArr), Object.prototype.toString.call(aicArr)],
    };
  });
  // A concatenated string arrived as a JavaObject; which flattening keeps it a
  // JS string once written to the scope? Values read from nodeState, headers
  // and cookies arrive as plain strings, so this only matters for computed ones.
  p.flatten = attempt(() => {
    var cat = "c-" + p.run;
    lib.inject(self, {
      aicCat: cat,
      aicCatString: String(cat),
      aicCatToString: cat.toString(),
      aicCatSlice: cat.slice(0),
      aicCatTemplate: `${cat}`,
      aicFromState: nodeState.get("myValue1"),
    });
    return {
      cat: typeof aicCat,
      String: typeof aicCatString,
      toString: typeof aicCatToString,
      slice: typeof aicCatSlice,
      template: typeof aicCatTemplate,
      fromState: [typeof aicFromState, aicFromState === "hello-" + p.run],
    };
  });
  // Does a getter defined on the scope avoid the conversion?
  p.getter = attempt(() => {
    Object.defineProperty(self, "aicGet", {
      get: () => false,
      configurable: true,
    });
    Object.defineProperty(self, "aicGetStr", {
      get: () => "x" + p.run,
      configurable: true,
    });
    return {
      falseVal: [typeof aicGet, aicGet ? "truthy" : "falsy"],
      str: [typeof aicGetStr, aicGetStr === "x" + p.run],
    };
  });

  // Option B for comparison.
  p.optionB = attempt(() =>
    lib.runWith({ myValue1: "arg-" + p.run }, (v) => String(v.myValue1))
  );

  p.ok = true;
} catch (e) {
  p.ok = false;
  p.error = String(e);
}

emit(p);
