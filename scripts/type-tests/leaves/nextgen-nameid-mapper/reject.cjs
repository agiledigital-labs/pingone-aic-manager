identity.store(); // expect: TS2339 — not measured; omitted from the declaration
identity.setAttribute("mail", ["a"]); // expect: TS2339 — not measured
identity.addAttribute("mail", "a"); // expect: TS2339 — not measured
identity.getUniversalId(); // expect: TS2339 — not measured
session.getName(); // expect: TS18046 — no session member was measured
nameIDScriptHelper.getNameIDFromSession().toString(); // expect: TS2531 — the only measured return is null
nameIDScriptHelper.createNameIdentifier(); // expect: TS18046 — call signature unmeasured
nameIDScriptHelper.NAMEID_FORMAT_EMAIL; // expect: TS2339 — constant unmeasured
identity.getAttributeValues("uid"); // expect: TS2345 — only mail measured
identity.getAttributeValues("mail").size(); // expect: TS2339 — only length and index zero measured
identity.getAttributeValues("mail").push("x"); // expect: TS2339 — mutation unmeasured
