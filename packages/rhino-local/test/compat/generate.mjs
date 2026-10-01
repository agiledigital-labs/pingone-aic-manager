/* global URL, console */
import ts from "typescript";
// Writes consumer-0.1.2.ts from the vendored 0.1.2 declarations:
//   node test/compat/generate.mjs > test/compat/consumer-0.1.2.ts
const root = new URL("./v0.1.2", import.meta.url).pathname;
// Types only the harness builds and hands to consumer code. A richer value is
// fine there, so these are checked HEAD -> 0.1.2 rather than 0.1.2 -> HEAD;
// chainFromRunResult takes RunResultInput, which is checked the other way
// through its call below.
const HEADER = "/**\n * A 0.1.2-shaped consumer, compiled against HEAD by `npm run typecheck`.\n *\n * `v0.1.2/` is the declaration output of tag `script-tester-v0.1.2`, vendored\n * as built apart from the lines marked `compat-accepted`. Every line below\n * takes a value typed by 0.1.2 and hands it to the\n * current export of the same name: every exported function and method is\n * called with 0.1.2's parameter list, and every exported interface and type\n * (callback interfaces included) is assigned from its 0.1.2 shape. A\n * compile error here is a source break for a 0.1.2 consumer. Nothing in this\n * file runs; it is not a `*.test.ts`.\n *\n * Types only the harness builds (OUTPUTS in generate.mjs) are checked the\n * other way, HEAD -> 0.1.2: a richer value is fine there. Lines marked\n * `compat-accepted` in v0.1.2/ are the deliberate source changes since 0.1.2,\n * each documented in the README. Regenerate with `generate.mjs`.\n *\n * Generic parameters are filled with `any`, so this checks each type's\n * structure rather than its schema-specific inference.\n */\n/* eslint-disable @typescript-eslint/no-explicit-any */\nimport type * as OldHarness from \"./v0.1.2/src/harness/index.ts\";\nimport type * as OldCase from \"./v0.1.2/src/case/index.ts\";\nimport type * as OldAic from \"./v0.1.2/src/aic/index.ts\";\nimport type * as OldBindings from \"./v0.1.2/src/bindings/index.ts\";\nimport type * as OldRunner from \"./v0.1.2/src/runner.ts\";\nimport type * as OldProfile from \"./v0.1.2/src/profile/index.ts\";\nimport type * as OldDiagnostics from \"./v0.1.2/src/diagnostics.ts\";\nimport * as NewHarness from \"../../src/harness/index.ts\";\nimport * as NewCase from \"../../src/case/index.ts\";\nimport * as NewAic from \"../../src/aic/index.ts\";\nimport * as NewBindings from \"../../src/bindings/index.ts\";\nimport * as NewRunner from \"../../src/runner.ts\";\nimport * as NewProfile from \"../../src/profile/index.ts\";\nimport * as NewDiagnostics from \"../../src/diagnostics.ts\";\n\n";
const OUTPUTS = new Set([
  "RunResult", "StepResult", "CheckContext", "StepContext", "BeforeRunContext",
  "CaseRun", "LeaseLaneRunRequest",
]);
const entries = [
  ["src/harness/index", "Harness"], ["src/case/index", "Case"], ["src/aic/index", "Aic"],
  ["src/bindings/index", "Bindings"], ["src/runner", "Runner"], ["src/profile/index", "Profile"],
  ["src/diagnostics", "Diagnostics"],
];
const files = entries.map(([e]) => `${root}/${e}.d.ts`);
const program = ts.createProgram(files, { allowImportingTsExtensions: true, noEmit: true, module: ts.ModuleKind.NodeNext, moduleResolution: ts.ModuleResolutionKind.NodeNext, skipLibCheck: true });
const checker = program.getTypeChecker();
const F = ts.SymbolFlags;
const out = [
  "type Keeps<O, N> = O extends readonly unknown[]",
  "  ? N extends readonly unknown[] ? (O[number] extends N[number] ? true : false) : false",
  "  : N extends O ? true : false;",
];
for (const [i, f] of files.entries()) {
  const [path, alias] = entries[i];
  const sf = program.getSourceFile(f);
  const sym = checker.getSymbolAtLocation(sf);
  out.push(`\n// ${path}`);
  for (const ex of checker.getExportsOfModule(sym)) {
    const s = ex.flags & F.Alias ? checker.getAliasedSymbol(ex) : ex;
    const n = ex.name;
    const O = `Old${alias}`, N = `New${alias}`, id = `_${alias}_${n}`;
    const d = s.declarations?.[0];
    const tps = d?.typeParameters ? `<${d.typeParameters.map(() => "any").join(", ")}>` : "";
    if (s.flags & F.Class) {
      const ctor = s.members?.get("__constructor");
      const priv = ctor?.declarations?.some(c => ts.getCombinedModifierFlags(c) & (ts.ModifierFlags.Private|ts.ModifierFlags.Protected));
      if (!priv && ctor) out.push(`declare const ${id}__new: ConstructorParameters<typeof ${O}.${n}>;\nvoid new ${N}.${n}(...${id}__new);`);
      const inst = checker.getDeclaredTypeOfSymbol(s);
      out.push(`declare const ${id}__inst: ${N}.${n}${tps};`);
      for (const p of checker.getPropertiesOfType(inst)) {
        const pd = p.declarations?.[0];
        if (!pd || ts.getCombinedModifierFlags(pd) & (ts.ModifierFlags.Private|ts.ModifierFlags.Protected) || p.name.startsWith("#")) continue;
        if (p.flags & F.Method) {
          out.push(`declare const ${id}__${p.name}: Parameters<${O}.${n}${tps}["${p.name}"]>;\nvoid ${id}__inst.${p.name}(...${id}__${p.name});`);
        }
      }
      for (const p of checker.getPropertiesOfType(checker.getTypeOfSymbol(s))) {
        if (!p.declarations?.some(pd => pd.parent === d)) continue;
        if (p.flags & F.Method) out.push(`declare const ${id}__s_${p.name}: Parameters<typeof ${O}.${n}["${p.name}"]>;\nvoid ${N}.${n}.${p.name}(...${id}__s_${p.name});`);
      }
    } else if (s.flags & F.Function) {
      out.push(`declare const ${id}: Parameters<typeof ${O}.${n}>;\nvoid ${N}.${n}(...${id});`);
    } else if (s.flags & F.Variable) {
      // An exported constant is read, never supplied; a tuple may gain entries
      // but must keep every 0.1.2 one.
      out.push(`export const ${id}_ok: Keeps<typeof ${O}.${n}, typeof ${N}.${n}> = true;`);
    } else if (s.flags & (F.Interface | F.TypeAlias)) {
      out.push(OUTPUTS.has(n)
        ? `declare const ${id}: ${N}.${n}${tps};\nexport const ${id}_ok: ${O}.${n}${tps} = ${id};`
        : `declare const ${id}: ${O}.${n}${tps};\nexport const ${id}_ok: ${N}.${n}${tps} = ${id};`);
    } else out.push(`// unhandled ${n}`);
  }
}
console.log(HEADER + out.join("\n"));
