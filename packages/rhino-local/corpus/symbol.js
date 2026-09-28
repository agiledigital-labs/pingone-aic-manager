// rhino-local-corpus
// row: symbol
// cite: docs/api/12-script-bindings-matrix.md | ES2015 global objects | Symbol
// probed: 2026-07-30
// aic-compiled: true
// aic-evaluated: true
// aic-result: typeof=undefined
// aic-summary: typeof undefined
// verdict: check
//
// Symbol is a function, not `new Symbol()`. AIC recorded typeof only plus
// the family-wide `new Map()` ReferenceError; we do not invent a `new Symbol`
// expectation.

__result = "typeof=" + typeof Symbol;
