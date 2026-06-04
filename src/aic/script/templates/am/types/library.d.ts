// Library script bindings. Library scripts are next-generation CommonJS modules:
// they have the common next-gen bindings (see common.d.ts) PLUS the CommonJS
// module mechanics below. Only next-generation scripts can require() libraries,
// and libraries may require other libraries (verified by corpus usage —
// 17 sandbox libraries require others).
//
// Libraries receive per-call state (e.g. nodeState) as function arguments via
// the common `.load(...)` factory pattern, so the scripted-decision globals are
// intentionally NOT in scope here. (`require` comes from nextgen-common.d.ts.)

declare const module: { exports: any };
declare const exports: any;
