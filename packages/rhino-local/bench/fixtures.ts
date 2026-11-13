/**
 * Invented benchmark fixtures. Nothing here is derived from any client code:
 * the library bodies are generated filler shaped like a typical AM library
 * map (many small modules plus one `lib-common` that pulls most of them in).
 */

export const LIBRARY_COUNT = 80;
/** How many of the generated libraries `lib-common` requires. */
export const COMMON_FANOUT = 60;

function libraryBody(index: number): string {
  const lines: string[] = [
    `// lib-${pad(index)}: generated benchmark filler`,
    "var counter = 0;",
  ];
  for (let f = 0; f < 12; f += 1) {
    lines.push(
      `function helper${f}(input) {`,
      `  // Normalises ${f} for the benchmark; deliberately unremarkable code.`,
      `  var out = [];`,
      `  for (var i = 0; i < ${f + 2}; i += 1) {`,
      `    out.push(String(input) + "-${index}-${f}-" + i);`,
      `  }`,
      `  counter += 1;`,
      `  return out.join("|").length + ${f};`,
      `}`,
      `exports.helper${f} = helper${f};`
    );
  }
  lines.push(`exports.value = function () { return ${index}; };`);
  return lines.join("\n");
}

function pad(n: number): string {
  return String(n).padStart(3, "0");
}

/** ~290 KB of library source keyed by require() id. */
export function benchLibraries(): Record<string, string> {
  const libs: Record<string, string> = {};
  for (let i = 0; i < LIBRARY_COUNT; i += 1) {
    libs[`lib-${pad(i)}`] = libraryBody(i);
  }
  const requires: string[] = [];
  for (let i = 0; i < COMMON_FANOUT; i += 1) {
    requires.push(`var m${i} = require("lib-${pad(i)}");`);
  }
  libs["lib-common"] = [
    ...requires,
    "exports.sum = function (n) {",
    "  var total = 0;",
    ...Array.from({ length: COMMON_FANOUT }, (_, i) => `  total += m${i}.value();`),
    "  return total + n;",
    "};",
  ].join("\n");
  return libs;
}

export const USES_COMMON =
  'var common = require("lib-common");\nnodeState.putShared("n", common.sum(3));\naction.goTo("done");';
export const NO_REQUIRE = 'nodeState.putShared("n", 1);\naction.goTo("done");';
/** sum(3) over lib-000..lib-059 values. */
export const COMMON_SUM = (COMMON_FANOUT * (COMMON_FANOUT - 1)) / 2 + 3;
