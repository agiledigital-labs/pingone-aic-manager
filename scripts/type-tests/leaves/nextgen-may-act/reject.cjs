token.getMayAct().toString(); // expect: TS2531 — JS null when the claim is absent
token.getAct().toString(); // expect: TS2531 — JS null when the claim is absent
token.setField("a", 1); // expect: TS2551 — absent on this wrapper (setAct is the near miss)
scopes.map(function (s) { return s; }); // expect: TS2339 — a Java list has no map
// policy: not a known runtime failure; get(i) is an unmeasured member, left undeclared.
scopes.get(0); // expect: TS2339 — policy: use scopes[0]
// policy: not a known runtime failure; add() is an unmeasured member here, left undeclared.
scopes.add("x"); // expect: TS2339 — policy: add() was not measured on may-act scopes
