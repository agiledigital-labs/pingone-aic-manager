config = "a message"; // expect: TS2322 — AM needs a Map: this is a 500
config = 42; // expect: TS2322 — AM needs a Map: this is a 500
nodeState.keys(); // expect: TS2339 — the class shutter on next-gen
outcome = "true"; // expect: TS2304 — not bound
identity; // expect: TS2304 — not bound
cacheManager; // expect: TS2304 — not bound
