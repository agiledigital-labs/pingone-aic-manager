logger.error("two {} and {} placeholders", "alpha"); // expect: TS2345 — deficit
logger.error("three {} {} {}", "a", "b"); // expect: TS2345 — deficit
logger.message("one {} placeholder", "alpha", "bravo"); // expect: TS2345 — surplus non-throwable
logger.warning("no placeholders at all", "alpha"); // expect: TS2345 — surplus non-throwable
logger.error("escaped \\{} binds nothing", "alpha"); // expect: TS2345 — escaped, so no slot
logger.error("double \\\\{} bound", "a", "b"); // expect: TS2345 — surplus; the ACCEPT row is what proves parity
logger.error("boom", undefined); // expect: TS2345 — a dropped argument, not a throwable
accessToken.setAct({ sub: "x" }); // expect: TS2551 — not callable here; tsc suggests getAct
accessToken.setPermissions({}); // expect: TS2551 — next-gen only; tsc suggests getPermissions
identity.getAttributeValues("mail"); // expect: TS2551 — next-gen Identity spelling; tsc suggests getAttribute
identity.exists(); // expect: TS2551 — legacy spelling is isExists(), which tsc suggests
secrets.getGenericSecret("x"); // expect: TS2304 — binding is undefined here
accessToken.setScope(["openid"]); // expect: TS2345 — needs a java.util.Set
scopes.length; // expect: TS2339 — a java.util.Set has no length
identity.getAttribute("mail").length; // expect: TS2339 — getAttribute returns a java.util.HashSet
identity.getAttribute("mail").get(0); // expect: TS2339 — HashSet has no get(int); use toArray()[0]
// NOT a reject row: `identity.getAttribute("mail")[0]` throws at runtime
// (`java.util.HashSet has no public instance field or method named "0"`) and
// still compiles clean. These fixtures are `.cjs` under `checkJs`, where tsc
// does not raise the implicit-any element-access error, so numeric indexing of
// ANY JavaSet — `scopes[0]` included — is invisible to the type system. Named
// here so the gap is not rediscovered as a bug in these declarations.
identity.getAttributes().length; // expect: TS2339 — getAttributes returns a Map, not a list
accessToken.getNonce().split(""); // expect: TS2531 — the getter is nullable and must be checked
