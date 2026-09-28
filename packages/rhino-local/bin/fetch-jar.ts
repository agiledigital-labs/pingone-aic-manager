#!/usr/bin/env node
/**
 * Download and verify the pinned Rhino jar into the cache, and print its path.
 *
 *   npx aic-script-tester-fetch-jar
 *
 * For machines that will run offline later, or CI images that pre-seed a
 * cache. The jar is not bundled with the package (it is MPL-2.0 and 1.4 MB);
 * a machine that cannot reach Maven Central can point RHINO_LOCAL_RHINO_JAR at
 * a copy instead, which is verified by the same SHA-256.
 */
import { ensureRhinoJar } from "../src/jvm.ts";

process.stdout.write(`${await ensureRhinoJar()}\n`);
