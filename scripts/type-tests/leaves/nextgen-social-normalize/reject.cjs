rawProfile.sub; // expect: TS4111 — dot access to a profile key; use get("sub") or ["sub"]
nodeState.keys(); // expect: TS2339 — the class shutter on next-gen
sharedState; // expect: TS2304 — legacy only
transientState; // expect: TS2304 — legacy only
normalizedProfile; // expect: TS2304 — the handler node's binding, not this one's
outcome = "true"; // expect: TS2304 — not bound
existingSession.valueOf(); // expect: TS18048 — a ReferenceError without a session cookie
