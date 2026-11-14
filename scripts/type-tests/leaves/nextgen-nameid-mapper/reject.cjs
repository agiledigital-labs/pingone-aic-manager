identity.store(); // expect: TS2339 — not measured; omitted from the declaration
identity.setAttribute("mail", ["a"]); // expect: TS2339 — not measured
identity.addAttribute("mail", "a"); // expect: TS2339 — not measured
identity.getUniversalId(); // expect: TS2339 — not measured
session.getName(); // expect: TS2339 — bound as object; no member was read
nameIDScriptHelper.getNameIDFromSession().toString(); // expect: TS2531 — the only measured return is null
