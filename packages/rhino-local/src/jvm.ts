import { execFile } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  lstatSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { delimiter, join } from "node:path";
import { promisify } from "node:util";
import { javaSourceDir } from "./paths.ts";

const execFileAsync = promisify(execFile);

/**
 * Where the runner's JVM comes from.
 *
 * - `host` — a Java 25 on this machine. The default: an 8GB laptop should not
 *   need the ~850MB AM image to run a unit test.
 * - `container` — the JVM inside the AM image. The reference `host` is
 *   compared against; CI runs it, developers need not.
 * - `both` — every job on both, failing on any difference. This is how the
 *   whole suite doubles as the host-vs-image conformance test.
 */
export type JvmLane = "host" | "container" | "both";

export const JVM_LANE_ENV = "RHINO_LOCAL_JVM";

export function laneFromEnv(env: NodeJS.ProcessEnv = process.env): JvmLane {
  const raw = env[JVM_LANE_ENV];
  if (raw === undefined || raw === "" || raw === "host") {
    return "host";
  }
  if (raw === "container" || raw === "both") {
    return raw;
  }
  throw new Error(
    `rhino-local: ${JVM_LANE_ENV} must be host, container or both, not ${JSON.stringify(raw)}`
  );
}

/**
 * The Rhino AM 8.1.1 ships. MEASURED 2026-09-28: the jar in
 * `WEB-INF/lib` of the AM image is byte-identical to this Maven Central
 * artefact (same SHA-1 as the `.sha1` Central publishes), so there is no Ping
 * code to extract and nothing about Rhino that needs the image.
 */
export const RHINO_JAR = {
  fileName: "rhino-1.7.14.1.jar",
  url: "https://repo1.maven.org/maven2/org/mozilla/rhino/1.7.14.1/rhino-1.7.14.1.jar",
  sha256: "407d4af6521494061d86b208e99804e0bf5c2fa38449c48f24b2f5a7154b4d8d",
  bytes: 1_389_188,
} as const;

/**
 * The AM image the `container` lane runs, pinned by digest so the reference
 * cannot move under a re-pushed tag; override to calibrate another image.
 */
export const DEFAULT_AM_IMAGE =
  "us-docker.pkg.dev/forgeops-public/images/am:2026.3.1-2053" +
  "@sha256:358d7e1e13b27619b742a759fd5a85d4fcc57cc75811027b9f3c0019a0bd9be3";

/**
 * The JVM AM runs, as the AM image reports it (MEASURED 2026-09-28):
 * Temurin-25.0.4+7, `/etc/localtime` → `Etc/UTC`, `LANG=en_US.UTF-8`. The
 * runner reports what it actually got at startup, and {@link checkEnvironment}
 * refuses a JVM that differs in anything a script could observe.
 */
export const EXPECTED_ENVIRONMENT = {
  javaFeature: 25,
  timezone: "Etc/UTC",
  locale: "en-US",
  charset: "UTF-8",
  rhino: "Rhino 1.7.14.1",
} as const;

/**
 * Flags for the host lane only. The properties pin what the AM image's JVM
 * gets from its OS (UTC, en_US.UTF-8), which a host would otherwise take from
 * its own settings. The heap cap, serial collector and C1-only JIT are what
 * keep a runner small on an 8GB box.
 *
 * The container lane gets **none** of these: it is the reference, so it runs
 * the image's JVM on the image's defaults, as the old launcher did. That makes
 * `both` compare the constrained host against an unconstrained AM JVM, so a
 * script that only works with more heap or a faster JIT shows up as a
 * divergence rather than agreeing with itself.
 */
export const HOST_JVM_FLAGS: readonly string[] = [
  "-Duser.timezone=Etc/UTC",
  "-Duser.language=en",
  "-Duser.country=US",
  "-Dfile.encoding=UTF-8",
  "-Dstdout.encoding=UTF-8",
  "-Dstderr.encoding=UTF-8",
  "-Xmx256m",
  "-XX:+UseSerialGC",
  "-XX:TieredStopAtLevel=1",
];

export interface RunnerEnvironment {
  javaFeature: number;
  javaVersion: string;
  javaVendorVersion: string | null;
  timezone: string;
  locale: string;
  charset: string;
  rhino: string;
}

export function parseEnvironment(readyLine: string): RunnerEnvironment {
  const json = readyLine.replace(/^.*?rhino-local-runner ready\s*/, "");
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    throw new Error(
      `rhino-local: the runner's ready line carries no environment (${JSON.stringify(readyLine)}); ` +
        "its classes predate this client — delete the rhino-local cache"
    );
  }
  if (typeof raw !== "object" || raw === null) {
    throw new Error(`rhino-local: runner environment is not an object: ${json}`);
  }
  const r = raw as Record<string, unknown>;
  const str = (key: string): string => {
    const value = r[key];
    if (typeof value !== "string") {
      throw new Error(`rhino-local: runner environment is missing ${key}: ${json}`);
    }
    return value;
  };
  if (typeof r.javaFeature !== "number") {
    throw new Error(`rhino-local: runner environment is missing javaFeature: ${json}`);
  }
  return {
    javaFeature: r.javaFeature,
    javaVersion: str("javaVersion"),
    javaVendorVersion: typeof r.javaVendorVersion === "string" ? r.javaVendorVersion : null,
    timezone: str("timezone"),
    locale: str("locale"),
    charset: str("charset"),
    rhino: str("rhino"),
  };
}

/**
 * Every field a script can observe must match AM's. The Java patch level and
 * vendor are not checked here: they are what the `both` lane measures, and a
 * developer's Temurin 25.0.4.1 is not wrong merely for not being 25.0.4.
 */
export function checkEnvironment(env: RunnerEnvironment, lane: "host" | "container"): void {
  const problems: string[] = [];
  for (const key of Object.keys(EXPECTED_ENVIRONMENT) as Array<keyof typeof EXPECTED_ENVIRONMENT>) {
    if (env[key] !== EXPECTED_ENVIRONMENT[key]) {
      problems.push(
        `${key} is ${JSON.stringify(env[key])}, AM's is ${JSON.stringify(EXPECTED_ENVIRONMENT[key])}`
      );
    }
  }
  if (problems.length > 0) {
    const remedy =
      lane === "host" && env.javaFeature !== EXPECTED_ENVIRONMENT.javaFeature
        ? ` Point ${JAVA_HOME_ENV} (or JAVA_HOME) at a Java ${EXPECTED_ENVIRONMENT.javaFeature} — AM runs Temurin ${EXPECTED_ENVIRONMENT.javaFeature}.`
        : "";
    throw new Error(
      `rhino-local: the ${lane} JVM does not match AM: ${problems.join("; ")}.${remedy}`
    );
  }
}

export const JAVA_HOME_ENV = "RHINO_LOCAL_JAVA_HOME";

/** `bin/<tool>` under RHINO_LOCAL_JAVA_HOME, then JAVA_HOME, else the bare name for PATH lookup. */
export function javaTool(tool: "java" | "javac", env: NodeJS.ProcessEnv = process.env): string {
  const home = env[JAVA_HOME_ENV] || env.JAVA_HOME;
  return home ? join(home, "bin", tool) : tool;
}

export function cacheDir(env: NodeJS.ProcessEnv = process.env): string {
  if (env.RHINO_LOCAL_CACHE) {
    return env.RHINO_LOCAL_CACHE;
  }
  return join(env.XDG_CACHE_HOME || join(homedir(), ".cache"), "rhino-local");
}

function sha256(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

const javacVersions = new Map<string, Promise<string>>();

async function javacVersion(javac: string): Promise<string> {
  let version = javacVersions.get(javac);
  if (!version) {
    version = execFileAsync(javac, ["-version"], { timeout: 30_000, maxBuffer: 100_000 })
      .then(({ stdout, stderr }) => `${stdout}${stderr}`.trim())
      .catch((error) => {
        javacVersions.delete(javac);
        throw error;
      });
    javacVersions.set(javac, version);
  }
  return version;
}

function outputFiles(root: string, relative = ""): string[] {
  const dir = relative ? join(root, relative) : root;
  const files: string[] = [];
  for (const entry of readdirSync(dir)) {
    const child = relative ? join(relative, entry) : entry;
    const path = join(root, child);
    if (lstatSync(path).isDirectory()) files.push(...outputFiles(root, child));
    else files.push(child);
  }
  return files.sort();
}

/**
 * The first published directory for `digest` whose manifest still holds.
 * One that does not — a class deleted, altered or added since — is removed:
 * names are never reused, so no worker can be about to publish over it, and
 * a worker already running from it is running damaged classes regardless.
 */
function findValidClasses(classesDir: string, digest: string): string | undefined {
  let names: string[];
  try {
    names = readdirSync(classesDir);
  } catch {
    return undefined;
  }
  for (const name of names.filter((n) => n.startsWith(`${digest}-`)).sort()) {
    const dir = join(classesDir, name);
    if (validRunnerCache(dir, digest)) {
      return dir;
    }
    rmSync(dir, { recursive: true, force: true });
  }
  return undefined;
}

function validRunnerCache(dest: string, digest: string): boolean {
  const marker = join(dest, ".complete");
  if (!existsSync(marker)) return false;
  try {
    const lines = readFileSync(marker, "utf8").trimEnd().split("\n");
    if (lines[0] !== digest || lines.length < 2) return false;
    const entries = lines.slice(1);
    const listed = new Set<string>();
    let previous = "";
    for (const line of entries) {
      const match = /^([a-f0-9]{64})\x20{2}(.+)$/.exec(line);
      const relative = match?.[2];
      if (!match || !relative || relative <= previous || relative === ".complete") return false;
      previous = relative;
      listed.add(relative);
      if (sha256(readFileSync(join(dest, relative))) !== match[1]) return false;
    }
    return outputFiles(dest)
      .filter((file) => file.endsWith(".class"))
      .every((file) => listed.has(file));
  } catch {
    return false;
  }
}

export type Fetcher = (url: string) => Promise<Uint8Array>;

async function defaultFetch(url: string): Promise<Uint8Array> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`rhino-local: GET ${url} returned ${response.status}`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

/**
 * The Rhino jar, verified by SHA-256 every time it is used, not only when
 * downloaded: a cache is a file anyone can overwrite, and a different Rhino
 * would make every verdict in this harness a claim about the wrong engine.
 */
export async function ensureRhinoJar(
  cache: string = cacheDir(),
  fetcher: Fetcher = defaultFetch
): Promise<string> {
  const dir = join(cache, "jars");
  const path = join(dir, RHINO_JAR.fileName);
  if (existsSync(path)) {
    const actual = sha256(readFileSync(path));
    if (actual === RHINO_JAR.sha256) {
      return path;
    }
    rmSync(path, { force: true });
  }
  const bytes = await fetcher(RHINO_JAR.url);
  const actual = sha256(bytes);
  if (actual !== RHINO_JAR.sha256 || bytes.length !== RHINO_JAR.bytes) {
    throw new Error(
      `rhino-local: ${RHINO_JAR.url} is not the Rhino AM ships ` +
        `(sha256 ${actual}, ${bytes.length} bytes; expected ${RHINO_JAR.sha256}, ${RHINO_JAR.bytes})`
    );
  }
  mkdirSync(dir, { recursive: true });
  const tmp = join(dir, `.${RHINO_JAR.fileName}.${randomUUID()}`);
  writeFileSync(tmp, bytes);
  renameSync(tmp, path);
  return path;
}

/**
 * Compiled runner classes, keyed by the Java sources, the compiler and the jar
 * they compile against, so an edit to a `.java` file cannot run stale
 * bytecode. Each compile goes to a scratch directory and is renamed to a
 * **fresh** `<digest>-<uuid>` name, never to a shared one: Vitest spawns
 * runners from several workers at once, and a name that is never reused
 * cannot be replaced or moved after another worker has validated and returned
 * it. Two cold workers may both publish; the copies are identical and the
 * lowest valid name wins from then on.
 */
export async function ensureRunnerClasses(
  jar: string,
  options: { cache?: string; javac?: string; sources?: string } = {}
): Promise<string> {
  const cache = options.cache ?? cacheDir();
  const sourcesDir = options.sources ?? javaSourceDir;
  const sources = readdirSync(sourcesDir)
    .filter((name) => name.endsWith(".java"))
    .sort();
  if (sources.length === 0) {
    throw new Error(`rhino-local: no Java sources in ${sourcesDir}`);
  }
  const javac = options.javac ?? javaTool("javac");
  const compiler = await javacVersion(javac);
  const key = createHash("sha256");
  key.update(RHINO_JAR.sha256);
  key.update(`\0javac\0${compiler}`);
  for (const name of sources) {
    key.update(`\0${name}\0`);
    key.update(readFileSync(join(sourcesDir, name)));
  }
  const digest = key.digest("hex").slice(0, 16);
  const classesDir = join(cache, "classes");
  const found = findValidClasses(classesDir, digest);
  if (found !== undefined) {
    return found;
  }
  const scratch = join(classesDir, `.${digest}.${randomUUID()}`);
  mkdirSync(scratch, { recursive: true });
  try {
    try {
      await execFileAsync(
        javac,
        [
          "-encoding",
          "UTF-8",
          "--release",
          String(EXPECTED_ENVIRONMENT.javaFeature),
          "-cp",
          jar,
          "-d",
          scratch,
          ...sources.map((name) => join(sourcesDir, name)),
        ],
        { timeout: 120_000, maxBuffer: 4_000_000 }
      );
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      throw new Error(
        `rhino-local: compiling the runner with ${javac} failed. It needs a JDK ${EXPECTED_ENVIRONMENT.javaFeature} ` +
          `(set ${JAVA_HOME_ENV} or JAVA_HOME): ${detail}`
      );
    }
    const manifest = outputFiles(scratch).map(
      (file) => `${sha256(readFileSync(join(scratch, file)))}  ${file}`
    );
    writeFileSync(join(scratch, ".complete"), `${digest}\n${manifest.join("\n")}\n`);
    const dest = join(classesDir, `${digest}-${randomUUID()}`);
    renameSync(scratch, dest);
    return dest;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

export interface Launch {
  command: string;
  args: string[];
  env: NodeJS.ProcessEnv;
  /** Set for the container lane, so a crash can `docker rm -f` it. */
  containerName?: string;
}

export function hostLaunch(
  classes: string,
  jar: string,
  env: NodeJS.ProcessEnv = process.env
): Launch {
  return {
    command: javaTool("java", env),
    args: [...HOST_JVM_FLAGS, "-cp", `${classes}${delimiter}${jar}`, "Runner"],
    // TZ as well as -Duser.timezone: the property wins for java.util, but a
    // host TZ still leaks into anything that asks the OS.
    env: { ...env, TZ: "Etc/UTC" },
  };
}

export function containerLaunch(
  classes: string,
  jar: string,
  env: NodeJS.ProcessEnv = process.env
): Launch {
  const containerName = `rhino-local-runner-${randomUUID()}`;
  const image = env.RHINO_LOCAL_IMAGE || DEFAULT_AM_IMAGE;
  const user =
    typeof process.getuid === "function"
      ? [`--user`, `${process.getuid()}:${process.getgid?.() ?? 0}`]
      : [];
  return {
    command: "docker",
    args: [
      "run",
      "--rm",
      "-i",
      "--network",
      "none",
      "--name",
      containerName,
      ...user,
      "-v",
      `${classes}:/rhino-local/classes:ro`,
      "-v",
      `${jar}:/rhino-local/rhino.jar:ro`,
      "--entrypoint",
      "/opt/java/openjdk/bin/java",
      image,
      "-cp",
      "/rhino-local/classes:/rhino-local/rhino.jar",
      "Runner",
    ],
    env,
    containerName,
  };
}
