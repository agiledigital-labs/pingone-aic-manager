logger.message("claims for {} in {}", "alpha"); // expect: TS2345 — deficit
logger.error("no placeholders", "alpha"); // expect: TS2345 — surplus non-throwable
identity.getAttribute("mail").length; // expect: TS2339 — a java.util.HashSet has no length
identity.getAttribute("mail").get(0); // expect: TS2339 — no get(int); use toArray()[0]
identity.getAttributes().length; // expect: TS2339 — getAttributes returns a Map, not a list
identity.getAttributes(["mail"]); // expect: TS2345 — the overload takes a java.util.Set, not a JS array
