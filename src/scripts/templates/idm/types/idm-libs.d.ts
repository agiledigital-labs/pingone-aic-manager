// Bundled CommonJS libraries available to IDM (Rhino) scripts via
// `require('lib/<name>')`. These are baked into the IDM scripting runtime — you
// cannot push your own (see docs/api/11-idm-endpoints.md "Requireable bundled
// libraries"). Versions are pinned to the IDM 8.1 runtime, confirmed by the Ping
// scripting guide preface AND verified live against the sandbox 2026-06-22:
//
//   require('lib/lodash')    -> Lodash       3.10.1  (NOTE: v3, not v4)
//   require('lib/handlebars')-> Handlebars   4.7.7
//   require('lib/validator') -> validator.js 13.7.0
//
// The autocomplete here reflects the real runtime API. lodash and validator
// types come from `@types/lodash`/`@types/validator`, pinned in package.json to
// those exact runtime versions — the runtime packages themselves are NOT
// installed (they are never executed locally, only type-checked, and their old
// versions carry npm-audit CVEs). In particular lodash is v3: v4-only helpers
// (`_.fromPairs`, `_.toString`, the lazy-eval `_.chunk` changes, etc.) do NOT
// exist on the server and will (correctly) fail to type-check.
//
// Handlebars ships its own types, so `handlebars` IS installed (for the .d.ts
// only) — pinned to 4.7.9, an API-identical security patch of the runtime's
// 4.7.7. The 4.7.x type surface matches the server.
//
// Handlebars caveat (Ping scripting guide): using Handlebars in server-side JS
// requires wrapping calls in the Rhino Synchronizer, e.g.
//
//   var Handlebars = require('lib/handlebars');
//   var out = new Packages.org.mozilla.javascript.Synchronizer(function () {
//     return Handlebars.compile('Hi {{n}}')({ n: 'Dave' });
//   }, Handlebars)();

declare module "lib/lodash" {
  import _ = require("lodash");
  export = _;
}
declare module "lib/handlebars" {
  import Handlebars = require("handlebars");
  export = Handlebars;
}
declare module "lib/validator" {
  // @types/validator uses an ESM default export, but the Rhino runtime returns
  // the validator object directly (verified: `require('lib/validator').isEmail`
  // works), so normalise the default export to a CommonJS `export =`.
  import validator from "validator";
  export = validator;
}

// CommonJS `require`, typed for the three bundled libraries above. Anything else
// falls through to `any` — at runtime every other module id throws
// `Module "<id>" not found` (verified: 110+ npm names, all forms, 2026-06-22).
declare function require(id: "lib/lodash"): typeof import("lib/lodash");
declare function require(id: "lib/handlebars"): typeof import("lib/handlebars");
declare function require(id: "lib/validator"): typeof import("lib/validator");
declare function require(id: string): any;
