token.getMayAct().toString(); // expect: TS2531 — JS null when the claim is absent
token.getAct().toString(); // expect: TS2531 — JS null when the claim is absent
token.setField("a", 1); // expect: TS2551 — absent on this wrapper (setAct is the near miss)
scopes.map(function (s) { return s; }); // expect: TS2339 — a Java list has no map
