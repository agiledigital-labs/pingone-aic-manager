import { pathToFileURL } from "node:url";
import { buildPrebuiltClasses } from "./jvm.ts";

const entry = process.argv[1];
if (entry && import.meta.url === pathToFileURL(entry).href) {
  const out = await buildPrebuiltClasses();
  process.stdout.write(`wrote runner classes to ${out}\n`);
}
