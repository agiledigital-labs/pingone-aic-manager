// OIDC claims conformance probe: what AM emits for each value type, and how
// compositeScopes filters the id token and userinfo.
var nextGen = typeof require === "function";
if (nextGen) {
  ({
    values: {
      zzStr: "s", zzInt: 42, zzFloat: 1.5, zzBool: true, zzArr: ["a", "b"], zzObj: { a: 1, b: "x" }, zzNull: null,
      zzEmpty: "", zzScoped: "p", zzOther: "o", zzEmail: "e", zzUnscopedArr: [],
    },
    compositeScopes: { "zzemuc2-probe": ["zzScoped"], "zzemuc2-other": ["zzOther"], email: ["zzEmail"] },
  });
} else {
  var m = new java.util.LinkedHashMap();
  m.put("zzStr", "s"); m.put("zzInt", 42); m.put("zzFloat", 1.5); m.put("zzBool", true);
  m.put("zzArr", ["a", "b"]);
  var l = new java.util.ArrayList(); l.add("a"); l.add("b"); m.put("zzList", l);
  m.put("zzObj", { a: 1, b: "x" });
  var o = new java.util.LinkedHashMap(); o.put("a", 1); o.put("b", "x"); m.put("zzMap", o);
  m.put("zzNull", null); m.put("zzEmpty", "");
  m.put("zzScoped", "p"); m.put("zzOther", "o"); m.put("zzEmail", "e");
  var cs = new java.util.LinkedHashMap();
  var a1 = new java.util.ArrayList(); a1.add("zzScoped"); cs.put("zzemuc2-probe", a1);
  var a2 = new java.util.ArrayList(); a2.add("zzOther"); cs.put("zzemuc2-other", a2);
  var a3 = new java.util.ArrayList(); a3.add("zzEmail"); cs.put("email", a3);
  new org.forgerock.oauth2.core.UserInfoClaims(m, cs);
}
