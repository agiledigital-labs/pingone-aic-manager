config = "a message"; // expect: TS2322 — AM needs a Map: this is a 500
config = 42; // expect: TS2322 — AM needs a Map: this is a 500
config = []; // expect: TS2322 — an array is a 500 (AM needs a Map)
config = function () {}; // expect: TS2322 — a function is a 500 (AM needs a Map)
config = undefined; // expect: TS2322 — `undefined` is a 500
nodeState.keys(); // expect: TS2339 — the class shutter in this context
outcome = "true"; // expect: TS2304 — not bound
identity; // expect: TS2304 — not bound
cacheManager; // expect: TS2304 — not bound
