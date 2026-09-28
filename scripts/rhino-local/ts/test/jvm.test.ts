import {
  cpSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import {
  EXPECTED_ENVIRONMENT,
  RHINO_JAR,
  cacheDir,
  checkEnvironment,
  ensureRhinoJar,
  ensureRunnerClasses,
  laneFromEnv,
  parseEnvironment,
  type RunnerEnvironment,
} from "../src/jvm.ts";
import { javaSourceDir } from "../src/paths.ts";

const tempDirs: string[] = [];
function tempDir(): string {
  const path = mkdtempSync(join(tmpdir(), "rhino-local-jvm-test-"));
  tempDirs.push(path);
  return path;
}

afterEach(() => {
  for (const path of tempDirs.splice(0)) {
    rmSync(path, { recursive: true, force: true });
  }
});

describe("JVM configuration", () => {
  it.each([undefined, "", "host"])("selects host for %s", (lane) => {
    expect(laneFromEnv({ RHINO_LOCAL_JVM: lane })).toBe("host");
  });

  it.each(["container", "both"] as const)("selects %s", (lane) => {
    expect(laneFromEnv({ RHINO_LOCAL_JVM: lane })).toBe(lane);
  });

  it("rejects an unknown lane", () => {
    expect(() => laneFromEnv({ RHINO_LOCAL_JVM: "docker" })).toThrow("RHINO_LOCAL_JVM");
  });

  it("requires JSON environment data in the ready line", () => {
    expect(() => parseEnvironment("rhino-local-runner ready")).toThrow("rhino-local cache");
  });

  it("parses the runner environment", () => {
    const expected: RunnerEnvironment = {
      ...EXPECTED_ENVIRONMENT,
      javaVersion: "25.0.4+7",
      javaVendorVersion: "Temurin-25.0.4+7",
    };
    expect(parseEnvironment(`noise rhino-local-runner ready ${JSON.stringify(expected)}`)).toEqual(
      expected
    );
  });

  it("accepts an exact environment match", () => {
    const environment: RunnerEnvironment = {
      ...EXPECTED_ENVIRONMENT,
      javaVersion: "25.0.4+7",
      javaVendorVersion: "Temurin-25.0.4+7",
    };
    expect(() => checkEnvironment(environment, "host")).not.toThrow();
  });

  it.each(Object.keys(EXPECTED_ENVIRONMENT) as Array<keyof typeof EXPECTED_ENVIRONMENT>)(
    "reports an incorrect %s",
    (field) => {
      const environment: RunnerEnvironment = {
        ...EXPECTED_ENVIRONMENT,
        javaVersion: "25.0.4+7",
        javaVendorVersion: "Temurin-25.0.4+7",
      };
      Object.assign(environment, { [field]: field === "javaFeature" ? 24 : "wrong" });
      expect(() => checkEnvironment(environment, "container")).toThrow(field);
    }
  );

  it("points a host with the wrong Java feature at RHINO_LOCAL_JAVA_HOME", () => {
    const environment: RunnerEnvironment = {
      ...EXPECTED_ENVIRONMENT,
      javaFeature: 24,
      javaVersion: "24",
      javaVendorVersion: "Temurin-24",
    };
    expect(() => checkEnvironment(environment, "host")).toThrow("RHINO_LOCAL_JAVA_HOME");
  });
});

describe("ensureRhinoJar", () => {
  // The real jar, from the real cache (downloading it on a cold one), so the
  // "good" payload is the verified artefact and not a copy of the check.
  let goodJar: Buffer;
  beforeAll(async () => {
    goodJar = readFileSync(await ensureRhinoJar(cacheDir()));
  }, 120_000);

  it("uses a valid cached jar without fetching", async () => {
    const cache = tempDir();
    const path = join(cache, "jars", RHINO_JAR.fileName);
    mkdirSync(join(cache, "jars"), { recursive: true });
    writeFileSync(path, goodJar);
    const fetcher = async (): Promise<Uint8Array> => {
      throw new Error("fetcher should not run");
    };
    expect(await ensureRhinoJar(cache, fetcher)).toBe(path);
  });

  it("fetches, verifies, and writes a missing jar", async () => {
    const cache = tempDir();
    let calls = 0;
    const path = await ensureRhinoJar(cache, async (url) => {
      calls += 1;
      expect(url).toBe(RHINO_JAR.url);
      return new Uint8Array(goodJar);
    });
    expect(calls).toBe(1);
    expect(readFileSync(path)).toEqual(goodJar);
  });

  it("rejects wrong fetched bytes and leaves no jar behind", async () => {
    const cache = tempDir();
    await expect(ensureRhinoJar(cache, async () => new Uint8Array([1, 2, 3]))).rejects.toThrow(
      "is not the Rhino AM ships"
    );
    expect(() => readdirSync(join(cache, "jars"))).toThrow();
  });

  it("replaces a corrupted cached jar", async () => {
    const cache = tempDir();
    const dir = join(cache, "jars");
    mkdirSync(dir);
    const path = join(dir, RHINO_JAR.fileName);
    writeFileSync(path, "corrupt");
    let calls = 0;
    await ensureRhinoJar(cache, async () => {
      calls += 1;
      return new Uint8Array(goodJar);
    });
    expect(calls).toBe(1);
    expect(readFileSync(path)).toEqual(goodJar);
  });
});

describe("ensureRunnerClasses", () => {
  let jar: string;
  beforeAll(async () => {
    jar = await ensureRhinoJar(cacheDir());
  }, 120_000);

  function copySources(): { cache: string; sources: string } {
    const root = tempDir();
    const sources = join(root, "sources");
    mkdirSync(sources);
    cpSync(javaSourceDir, sources, {
      recursive: true,
      filter: (source) => source.endsWith(".java") || source === javaSourceDir,
    });
    return { cache: join(root, "cache"), sources };
  }

  it("reuses the compiled directory on the second call", async () => {
    const { cache, sources } = copySources();
    const first = await ensureRunnerClasses(jar, { cache, sources });
    const marker = join(first, ".complete");
    const modified = statSync(marker).mtimeMs;
    const second = await ensureRunnerClasses(jar, { cache, sources });
    expect(second).toBe(first);
    expect(statSync(marker).mtimeMs).toBe(modified);
  });

  it("uses a different directory when a source changes", async () => {
    const { cache, sources } = copySources();
    const first = await ensureRunnerClasses(jar, { cache, sources });
    const javaFile = join(sources, "Runner.java");
    writeFileSync(javaFile, `${readFileSync(javaFile, "utf8")}\n// source digest change\n`);
    const second = await ensureRunnerClasses(jar, { cache, sources });
    expect(second).not.toBe(first);
  });

  it("publishes one complete directory for concurrent cold calls", async () => {
    const { cache, sources } = copySources();
    const results = await Promise.all(
      Array.from({ length: 2 }, () => ensureRunnerClasses(jar, { cache, sources }))
    );
    const resolved = results[0];
    expect(resolved).toBeDefined();
    expect(resolved).toBe(results[1]);
    if (resolved === undefined) throw new Error("runner classes did not resolve");
    expect(readdirSync(resolved)).toContain(".complete");
    expect(readdirSync(join(cache, "classes"))).toEqual([basename(resolved)]);
  });
});
