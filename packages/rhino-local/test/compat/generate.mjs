import ts from "/home/dave/w/aic-script-tester-fixes-3/node_modules/typescript/lib/typescript.js";
const root = process.argv[2];
const entries = [
  ["src/harness/index", "Harness"], ["src/case/index", "Case"], ["src/aic/index", "Aic"],
  ["src/bindings/index", "Bindings"], ["src/runner", "Runner"], ["src/profile/index", "Profile"],
  ["src/diagnostics", "Diagnostics"],
];
const files = entries.map(([e]) => `${root}/${e}.d.ts`);
const program = ts.createProgram(files, { allowImportingTsExtensions: true, noEmit: true, module: ts.ModuleKind.NodeNext, moduleResolution: ts.ModuleResolutionKind.NodeNext, skipLibCheck: true });
const checker = program.getTypeChecker();
const F = ts.SymbolFlags;
const out = [];
for (const [i, f] of files.entries()) {
  const [path, alias] = entries[i];
  const sf = program.getSourceFile(f);
  const sym = checker.getSymbolAtLocation(sf);
  out.push(`\n// ${path}`);
  for (const ex of checker.getExportsOfModule(sym)) {
    const s = ex.flags & F.Alias ? checker.getAliasedSymbol(ex) : ex;
    const n = ex.name;
    const O = `Old${alias}`, N = `New${alias}`, id = `${alias}_${n}`;
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
      out.push(`export const ${id}_ok: typeof ${O}.${n} = ${N}.${n};`);
    } else if (s.flags & (F.Interface | F.TypeAlias)) {
      out.push(`declare const ${id}: ${O}.${n}${tps};\nexport const ${id}_ok: ${N}.${n}${tps} = ${id};`);
    } else out.push(`// unhandled ${n}`);
  }
}
console.log(out.join("\n"));
