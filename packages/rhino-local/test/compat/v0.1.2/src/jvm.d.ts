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
export declare const JVM_LANE_ENV = "AIC_SCRIPT_TESTER_JVM";
export declare function laneFromEnv(env?: NodeJS.ProcessEnv): JvmLane;
/**
 * The Rhino AM 8.1.1 ships. MEASURED 2026-09-28: the jar in
 * `WEB-INF/lib` of the AM image is byte-identical to this Maven Central
 * artefact (same SHA-1 as the `.sha1` Central publishes), so there is no Ping
 * code to extract and nothing about Rhino that needs the image.
 */
export declare const RHINO_JAR: {
    readonly fileName: "rhino-1.7.14.1.jar";
    readonly url: "https://repo1.maven.org/maven2/org/mozilla/rhino/1.7.14.1/rhino-1.7.14.1.jar";
    readonly sha256: "407d4af6521494061d86b208e99804e0bf5c2fa38449c48f24b2f5a7154b4d8d";
    readonly bytes: 1389188;
};
/**
 * The AM image the `container` lane runs, pinned by digest so the reference
 * cannot move under a re-pushed tag; override to calibrate another image.
 */
export declare const DEFAULT_AM_IMAGE: string;
/**
 * The JVM AM runs, as the AM image reports it (MEASURED 2026-09-28):
 * Temurin-25.0.4+7, `/etc/localtime` → `Etc/UTC`, `LANG=en_US.UTF-8`. The
 * runner reports what it actually got at startup, and {@link checkEnvironment}
 * refuses a JVM that differs in anything a script could observe.
 */
export declare const EXPECTED_ENVIRONMENT: {
    readonly javaFeature: 25;
    readonly timezone: "Etc/UTC";
    readonly locale: "en-US";
    readonly charset: "UTF-8";
    readonly rhino: "Rhino 1.7.14.1";
};
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
export declare const HOST_JVM_FLAGS: readonly string[];
export interface RunnerEnvironment {
    javaFeature: number;
    javaVersion: string;
    javaVendorVersion: string | null;
    timezone: string;
    locale: string;
    charset: string;
    rhino: string;
}
export declare function parseEnvironment(readyLine: string): RunnerEnvironment;
/**
 * Every field a script can observe must match AM's. The Java patch level and
 * vendor are not checked here: they are what the `both` lane measures, and a
 * developer's Temurin 25.0.4.1 is not wrong merely for not being 25.0.4.
 */
export declare function checkEnvironment(env: RunnerEnvironment, lane: "host" | "container"): void;
export declare const JAVA_HOME_ENV = "AIC_SCRIPT_TESTER_JAVA_HOME";
/** `bin/<tool>` under AIC_SCRIPT_TESTER_JAVA_HOME, then JAVA_HOME, else the bare name for PATH lookup. */
export declare function javaTool(tool: "java" | "javac", env?: NodeJS.ProcessEnv): string;
export declare function cacheDir(env?: NodeJS.ProcessEnv): string;
export type Fetcher = (url: string) => Promise<Uint8Array>;
/** A local copy of {@link RHINO_JAR} to use instead of the cache or a download. */
export declare const RHINO_JAR_ENV = "AIC_SCRIPT_TESTER_RHINO_JAR";
/**
 * The Rhino jar, verified by SHA-256 every time it is used, not only when
 * downloaded: a cache is a file anyone can overwrite, and a different Rhino
 * would make every verdict in this harness a claim about the wrong engine.
 */
export declare function ensureRhinoJar(cache?: string, fetcher?: Fetcher, env?: NodeJS.ProcessEnv): Promise<string>;
/**
 * The runner classes to launch. First the package's prebuilt classes, when
 * their manifest names these exact sources and every class still hashes as
 * recorded — an installed package, which may have no `javac` at all. Otherwise
 * classes compiled here, keyed by the Java sources, the compiler and the jar
 * they compile against, so an edit to a `.java` file cannot run stale
 * bytecode. Each compile goes to a scratch directory and is renamed to a
 * **fresh** `<digest>-<uuid>` name, never to a shared one: Vitest spawns
 * runners from several workers at once, and a name that is never reused
 * cannot be replaced or moved after another worker has validated and returned
 * it. Two cold workers may both publish; the copies are identical and the
 * lowest valid name wins from then on.
 */
export declare function ensureRunnerClasses(jar: string, options?: {
    cache?: string;
    javac?: string;
    sources?: string;
    prebuilt?: string;
}): Promise<string>;
/**
 * Compile the classes a package ships (`npm run build`). Their manifest is
 * keyed by the sources alone, not the compiler: the consumer running them has
 * no `javac` to key by, and `--release` fixes the class-file target.
 */
export declare function buildPrebuiltClasses(options?: {
    out?: string;
    javac?: string;
    sources?: string;
    cache?: string;
}): Promise<string>;
export interface Launch {
    command: string;
    args: string[];
    env: NodeJS.ProcessEnv;
    /** Set for the container lane, so a crash can `docker rm -f` it. */
    containerName?: string;
}
export declare function hostLaunch(classes: string, jar: string, env?: NodeJS.ProcessEnv): Launch;
export declare function containerLaunch(classes: string, jar: string, env?: NodeJS.ProcessEnv): Launch;
