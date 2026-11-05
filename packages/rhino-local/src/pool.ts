/**
 * Runner JVMs that outlive one test file.
 *
 * Vitest's default `isolate: true` runs every test file in a new worker
 * process, so a JVM a file spawns dies with it: a suite of a hundred files
 * started a hundred JVMs, each paying 0.3–0.7 s to start and running every
 * case cold. The pool lives in the Vitest main process instead (the package
 * reporter starts it) and lends each worker slot a JVM over a local socket:
 * the worker's {@link RhinoRunner} speaks the same line protocol it speaks
 * to a child's stdio, and when the file ends the JVM goes back to that slot
 * rather than exiting. One JVM per worker slot per lane, warm after the first
 * file.
 *
 * What reuse guarantees. Every job builds a fresh JavaScript scope
 * (`initStandardObjects()` and the whole preamble run again), and what the
 * JVM keeps between jobs — the compiled-script cache — is content-addressed
 * code, never state. So a script's JavaScript state never reaches another
 * job. Java-level state is a different matter: a script that can reach
 * `java.lang.System`, `java.util.TimeZone` or `java.lang.Thread` can change
 * the JVM itself. Behind the class shutter (a next-gen decision-node case,
 * the default) none of those classes is visible. Without it (the legacy
 * engine, `classShutter: false`, the IDM engine, or a raw `runner.eval` with
 * no `classAllowList`) they are, so:
 *
 * - a JVM that has run any job without a shutter is never lent to another
 *   client: it is discarded when its lease ends, and the next file starts a
 *   new one, as it would with no pool;
 * - within one lease (one file), cases share the JVM as they always have, so
 *   an unshuttered case can still leave JVM-global state for the next case in
 *   the same file.
 *
 * A JVM is also not lent again once it has exited or retired itself (a
 * spawned thread outlived its job), or while any job is still running in it.
 * Before a returned JVM goes idle the pool asks it (`op: "pool-status"`) and
 * discards it unless it is idle, not retired and has run only shuttered jobs.
 * A client that closes with a job pending kills its JVM outright. A JVM is
 * only reused by a client whose runner classes are the ones it was started
 * with (path and manifest), so rebuilding `java/*.java` in watch mode starts
 * fresh JVMs rather than testing the old build.
 * Opt out with `AIC_SCRIPT_TESTER_JVM_POOL=0`: each `useLease` then spawns and
 * closes its own JVM, as before.
 */
import { spawn, execFile, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { EventEmitter } from "node:events";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createConnection, createServer, type Server, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough, Writable } from "node:stream";
import { cacheDir, containerLaunch, ensureRhinoJar, ensureRunnerClasses, hostLaunch, type Launch } from "./jvm.ts";

/** Vitest provided-context key carrying the {@link PoolAddress} to workers. */
export const JVM_POOL_CONTEXT = "rhinoLocalJvmPool";
/** `0` turns the pool off. */
export const JVM_POOL_ENV = "AIC_SCRIPT_TESTER_JVM_POOL";

export interface PoolAddress {
  path: string;
  token: string;
}

/** The environment a worker's launch would have read; part of the pool key. */
const LAUNCH_ENV = [
  "PATH",
  "JAVA_HOME",
  "AIC_SCRIPT_TESTER_JAVA_HOME",
  "AIC_SCRIPT_TESTER_IMAGE",
  "AIC_SCRIPT_TESTER_RHINO_JAR",
] as const;

interface Hello {
  type: "hello";
  token: string;
  kind: "host" | "container";
  cache: string;
  slot: string;
  env: Partial<Record<(typeof LAUNCH_ENV)[number], string>>;
}

interface Status {
  type: typeof STATUS_OP;
  id: string;
  active: number;
  retired: boolean;
  /** The JVM has run a job with no class shutter; see the module comment. */
  unshuttered: boolean;
}

/** Frames the pool adds to the runner's stdout stream; the runner never prints this prefix. */
const CONTROL = '{"type":"rhino-local-pool"';
const READY = "rhino-local-runner ready";
const RETIRED = "rhino-local-runner retired";
const STATUS_OP = "pool-status";
const STATUS = '{"type":"pool-status"';
const STATUS_TIMEOUT_MS = 2_000;

/**
 * Whether a JVM's stderr has carried the retirement marker, read line by line
 * so a marker split across two `data` chunks is still seen. The status check
 * on return is the authority; this only stops lending sooner.
 */
export class RetirementWatch {
  #partial = "";
  retired = false;

  feed(chunk: string): void {
    const text = this.#partial + chunk;
    const lines = text.split("\n");
    // The last element is an unterminated line; keep only enough of it to finish a marker.
    this.#partial = (lines.pop() ?? "").slice(-RETIRED.length * 2);
    if (lines.some((line) => line.includes(RETIRED))) this.retired = true;
  }
}

export function poolEnabled(env: NodeJS.ProcessEnv = process.env): boolean {
  return env[JVM_POOL_ENV] !== "0";
}

/** One pooled runner JVM, owned by the main process. */
class PooledJvm {
  readonly key: string;
  /** The runner classes it was started with: see {@link buildIdentity}. */
  readonly build: string;
  readonly #watch = new RetirementWatch();
  #status: { id: string; resolve: (status: Status | undefined) => void } | undefined;
  readonly #child: ChildProcessWithoutNullStreams;
  readonly #containerName: string | undefined;
  #stdout = "";
  #readyLine: string | undefined;
  #readyWaiters: Array<() => void> = [];
  #stderr = "";
  #client: Socket | undefined;
  retired = false;
  exit: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  #exitWaiters: Array<() => void> = [];

  constructor(key: string, build: string, launch: Launch) {
    this.key = key;
    this.build = build;
    this.#containerName = launch.containerName;
    this.#child = spawn(launch.command, launch.args, { env: launch.env, stdio: ["pipe", "pipe", "pipe"] });
    this.#child.stdout.setEncoding("utf8");
    this.#child.stderr.setEncoding("utf8");
    this.#child.stdin.on("error", () => {
      // EPIPE after the JVM died; reported through exit.
    });
    this.#child.stdout.on("data", (chunk: string) => this.#onStdout(chunk));
    this.#child.stderr.on("data", (chunk: string) => this.#onStderr(chunk));
    this.#child.on("error", (error) => {
      this.#onStderr(`\n${error.message}\n`);
      this.#onExit(null, null);
    });
    this.#child.on("exit", (code, signal) => this.#onExit(code, signal));
    this.#unref();
  }

  get usable(): boolean {
    return this.exit === undefined && !this.retired && !this.#watch.retired;
  }

  /**
   * Ask the JVM whether it may be lent again: no job running, not retired,
   * and never ran a job without a class shutter. No answer within the bound,
   * or an answer missing a field, counts as no.
   */
  async reusable(): Promise<boolean> {
    if (!this.usable) return false;
    const id = `${STATUS_OP}-${randomUUID()}`;
    const status = await new Promise<Status | undefined>((resolve) => {
      const timer = setTimeout(() => finish(undefined), STATUS_TIMEOUT_MS);
      const finish = (value: Status | undefined): void => {
        clearTimeout(timer);
        if (this.#status?.id === id) this.#status = undefined;
        resolve(value);
      };
      this.#status = { id, resolve: finish };
      this.#exitWaiters.push(() => finish(undefined));
      this.write(JSON.stringify({ id, op: STATUS_OP }));
    });
    if (status?.retired === true) this.retired = true;
    return (
      status !== undefined &&
      status.active === 0 &&
      status.retired === false &&
      status.unshuttered === false &&
      this.usable
    );
  }

  attach(client: Socket): void {
    this.#client = client;
    this.#ref();
    // Only the ready line: a later client must not inherit an earlier one's diagnostics.
    void this.#whenReady().then(() => {
      if (this.#client !== client) return;
      if (this.#readyLine !== undefined) this.#send(client, { stderr: `${this.#readyLine}\n` });
      if (this.exit !== undefined) this.#send(client, { stderr: this.#stderr, exit: this.exit });
    });
  }

  detach(client: Socket): void {
    if (this.#client !== client) return;
    this.#client = undefined;
    this.#unref();
  }

  write(line: string): boolean {
    return this.exit === undefined ? this.#child.stdin.write(`${line}\n`) : true;
  }

  onDrain(fn: () => void): void {
    this.#child.stdin.once("drain", fn);
  }

  async stop(): Promise<void> {
    if (this.exit === undefined) {
      try {
        this.#child.stdin.write(`${JSON.stringify({ id: randomUUID(), op: "shutdown" })}\n`);
      } catch {
        // already closed
      }
      if (!(await this.#exited(7_000))) {
        this.#child.kill("SIGKILL");
        await this.#exited(2_000);
      }
    }
    await this.#removeContainer();
  }

  /** Also the synchronous last resort at process exit. */
  kill(): void {
    this.retired = true;
    if (this.exit === undefined) this.#child.kill("SIGKILL");
  }

  #whenReady(): Promise<void> {
    if (this.#readyLine !== undefined || this.exit !== undefined) return Promise.resolve();
    return new Promise((resolve) => this.#readyWaiters.push(resolve));
  }

  #exited(ms: number): Promise<boolean> {
    if (this.exit !== undefined) return Promise.resolve(true);
    return new Promise((resolve) => {
      const timer = setTimeout(() => resolve(false), ms);
      this.#exitWaiters.push(() => {
        clearTimeout(timer);
        resolve(true);
      });
    });
  }

  #onStdout(chunk: string): void {
    this.#stdout += chunk;
    let nl = this.#stdout.indexOf("\n");
    while (nl !== -1) {
      const line = this.#stdout.slice(0, nl + 1);
      this.#stdout = this.#stdout.slice(nl + 1);
      nl = this.#stdout.indexOf("\n");
      if (line.startsWith(STATUS)) {
        this.#onStatus(line);
        continue;
      }
      // A response for a client that has gone is dropped: its job ids mean nothing to the next.
      this.#client?.write(line);
    }
  }

  #onStatus(line: string): void {
    let status: Status | undefined;
    try {
      status = JSON.parse(line) as Status;
    } catch {
      status = undefined;
    }
    if (status !== undefined && this.#status?.id === status.id) this.#status.resolve(status);
  }

  #onStderr(chunk: string): void {
    if (this.#readyLine === undefined) {
      this.#stderr += chunk;
      const at = this.#stderr.indexOf(READY);
      const end = at === -1 ? -1 : this.#stderr.indexOf("\n", at);
      if (end !== -1) {
        this.#readyLine = this.#stderr.slice(at, end);
        this.#stderr = this.#stderr.slice(end + 1);
        for (const waiter of this.#readyWaiters.splice(0)) waiter();
      }
      return;
    }
    this.#watch.feed(chunk);
    // Kept short: it only feeds the exit message of whoever holds the JVM when it dies.
    this.#stderr = (this.#stderr + chunk).slice(-64_000);
    if (this.#client !== undefined) this.#send(this.#client, { stderr: chunk });
  }

  #onExit(code: number | null, signal: NodeJS.Signals | null): void {
    if (this.exit !== undefined) return;
    this.exit = { code, signal };
    for (const waiter of this.#readyWaiters.splice(0)) waiter();
    for (const waiter of this.#exitWaiters.splice(0)) waiter();
    const client = this.#client;
    if (client !== undefined && this.#readyLine !== undefined) {
      this.#send(client, { stderr: this.#stderr, exit: this.exit });
      client.end();
    }
  }

  #send(client: Socket, frame: Record<string, unknown>): void {
    client.write(`${JSON.stringify({ type: "rhino-local-pool", ...frame })}\n`);
  }

  // An idle pooled JVM must not keep the Vitest main process alive.
  #ref(): void {
    this.#child.ref();
    for (const stream of [this.#child.stdin, this.#child.stdout, this.#child.stderr]) (stream as unknown as { ref?: () => void }).ref?.();
  }

  #unref(): void {
    this.#child.unref();
    for (const stream of [this.#child.stdin, this.#child.stdout, this.#child.stderr]) (stream as unknown as { unref?: () => void }).unref?.();
  }

  async #removeContainer(): Promise<void> {
    if (this.#containerName === undefined) return;
    await new Promise<void>((resolve) => {
      execFile("docker", ["rm", "-f", this.#containerName as string], { timeout: 10_000 }, () => resolve());
    });
  }
}

export interface PoolOptions {
  /**
   * The runner classes directory for a lend. Defaults to `ensureRunnerClasses`;
   * tests substitute a second build to prove a JVM is not reused across builds.
   * @internal
   */
  classes?: (jar: string, cache: string) => Promise<string>;
}

/** The main-process side: a socket server lending JVMs to worker slots. */
export class JvmPool {
  readonly address: PoolAddress;
  readonly #server: Server;
  readonly #dir: string;
  readonly #idle = new Map<string, PooledJvm[]>();
  readonly #all = new Set<PooledJvm>();
  #stopped = false;
  /** Status checks in progress, per key: a lend waits for them rather than spawning beside them. */
  readonly #returning = new Map<string, Set<Promise<void>>>();
  readonly #onExit = (): void => {
    for (const jvm of this.#all) jvm.kill();
    rmSync(this.#dir, { recursive: true, force: true });
  };

  readonly #classes: (jar: string, cache: string) => Promise<string>;

  private constructor(server: Server, dir: string, address: PoolAddress, options: PoolOptions) {
    this.#server = server;
    this.#dir = dir;
    this.address = address;
    this.#classes = options.classes ?? ((jar, cache) => ensureRunnerClasses(jar, { cache }));
  }

  static async start(options: PoolOptions = {}): Promise<JvmPool> {
    // A private directory: the socket admits whoever can open it, and it runs scripts.
    const dir = mkdtempSync(join(process.platform === "win32" ? tmpdir() : shortTmp(), "rhino-pool-"));
    const path =
      process.platform === "win32" ? `\\\\.\\pipe\\rhino-local-${randomUUID()}` : join(dir, "pool.sock");
    const token = randomBytes(24).toString("hex");
    const server = createServer();
    const pool = new JvmPool(server, dir, { path, token }, options);
    server.on("connection", (socket) => pool.#serve(socket));
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(path, () => {
        server.off("error", reject);
        resolve();
      });
    });
    server.unref();
    process.once("exit", pool.#onExit);
    return pool;
  }

  /** How many JVMs this pool has started; for tests and the benchmark. */
  get started(): number {
    return this.#all.size;
  }

  async stop(): Promise<void> {
    if (this.#stopped) return;
    this.#stopped = true;
    process.off("exit", this.#onExit);
    await new Promise<void>((resolve) => this.#server.close(() => resolve()));
    await Promise.all([...this.#all].map((jvm) => jvm.stop()));
    this.#all.clear();
    this.#idle.clear();
    rmSync(this.#dir, { recursive: true, force: true });
  }

  #serve(socket: Socket): void {
    socket.setEncoding("utf8");
    socket.setNoDelay(true);
    let buffer = "";
    let state: "hello" | "lending" | "attached" | "closed" = "hello";
    let jvm: PooledJvm | undefined;
    const queued: string[] = [];
    const fail = (message: string): void => {
      state = "closed";
      const frame = { type: "rhino-local-pool", stderr: `rhino-local: ${message}\n`, exit: { code: null, signal: null } };
      socket.end(`${JSON.stringify(frame)}\n`);
    };
    const forward = (held: PooledJvm, line: string): void => {
      if (line.startsWith(CONTROL)) {
        // The only control a client sends: kill the JVM it holds.
        held.kill();
        return;
      }
      if (!held.write(line)) {
        socket.pause();
        held.onDrain(() => socket.resume());
      }
    };
    socket.on("data", (chunk: string) => {
      buffer += chunk;
      for (let nl = buffer.indexOf("\n"); nl !== -1; nl = buffer.indexOf("\n")) {
        const line = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        if (state === "attached" && jvm !== undefined) {
          forward(jvm, line);
        } else if (state === "lending") {
          queued.push(line);
        } else if (state === "hello") {
          const hello = parseHello(line, this.address.token);
          if (hello === undefined) {
            fail("JVM pool handshake refused");
            return;
          }
          state = "lending";
          this.#lend(hello).then(
            (lent) => {
              if (state === "closed") {
                this.#return(lent);
                return;
              }
              jvm = lent;
              state = "attached";
              lent.attach(socket);
              for (const line of queued.splice(0)) forward(lent, line);
            },
            (error: unknown) => fail(error instanceof Error ? error.message : String(error))
          );
        }
      }
    });
    socket.on("close", () => {
      state = "closed";
      // A partial line from a client that went away is dropped, never forwarded.
      buffer = "";
      if (jvm !== undefined) {
        jvm.detach(socket);
        this.#return(jvm);
        jvm = undefined;
      }
    });
    socket.on("error", () => {
      // close follows
    });
  }

  async #lend(hello: Hello): Promise<PooledJvm> {
    const key = JSON.stringify([hello.kind, hello.cache, hello.slot, hello.env]);
    await Promise.all([...(this.#returning.get(key) ?? [])]);
    if (this.#stopped) throw new Error("JVM pool is stopped");
    const env: NodeJS.ProcessEnv = { ...process.env };
    for (const name of LAUNCH_ENV) {
      if (hello.env[name] === undefined) delete env[name];
      else env[name] = hello.env[name];
    }
    // Content-addressed and cheap once built; asked on every lend so an edited
    // runner is never served by a JVM started from the previous build.
    const jar = await ensureRhinoJar(hello.cache, undefined, env);
    const classes = await this.#classes(jar, hello.cache);
    const build = buildIdentity(classes);
    const idle = this.#idle.get(key) ?? [];
    while (idle.length > 0) {
      const jvm = idle.pop() as PooledJvm;
      if (jvm.usable && jvm.build === build) return jvm;
      this.#discard(jvm);
    }
    const launch = hello.kind === "host" ? hostLaunch(classes, jar, env) : containerLaunch(classes, jar, env);
    const jvm = new PooledJvm(key, build, launch);
    this.#all.add(jvm);
    return jvm;
  }

  #return(jvm: PooledJvm): void {
    if (this.#stopped || !jvm.usable) {
      this.#discard(jvm);
      return;
    }
    const checks = this.#returning.get(jvm.key) ?? new Set<Promise<void>>();
    this.#returning.set(jvm.key, checks);
    const check = jvm.reusable().then((reusable) => {
      checks.delete(check);
      if (checks.size === 0) this.#returning.delete(jvm.key);
      if (this.#stopped || !reusable) {
        // Still running a job, retired, or it ran unshuttered: never lend it again.
        jvm.kill();
        this.#discard(jvm);
        return;
      }
      const idle = this.#idle.get(jvm.key) ?? [];
      idle.push(jvm);
      this.#idle.set(jvm.key, idle);
    });
    checks.add(check);
  }

  #discard(jvm: PooledJvm): void {
    this.#all.delete(jvm);
    void jvm.stop();
  }
}

/**
 * Which build a classes directory holds: its path and its manifest
 * (`.complete`, a hash of every class file). The path alone is not enough:
 * the package's prebuilt `dist/classes` keeps its path when `npm run build`
 * rewrites it, so a JVM started from the old classes would be reused.
 */
function buildIdentity(classes: string): string {
  let manifest: string;
  try {
    manifest = createHash("sha256").update(readFileSync(join(classes, ".complete"))).digest("hex");
  } catch {
    // No manifest to read: a fresh identity, so nothing is reused on a guess.
    manifest = `unread-${randomUUID()}`;
  }
  return `${classes}\0${manifest}`;
}

function parseHello(line: string, token: string): Hello | undefined {
  try {
    const hello = JSON.parse(line) as Partial<Hello>;
    if (
      hello.type !== "hello" ||
      hello.token !== token ||
      (hello.kind !== "host" && hello.kind !== "container") ||
      typeof hello.cache !== "string" ||
      typeof hello.slot !== "string" ||
      typeof hello.env !== "object" ||
      hello.env === null
    ) {
      return undefined;
    }
    return hello as Hello;
  } catch {
    return undefined;
  }
}

/** Unix socket paths are limited to ~104 bytes; a long TMPDIR would exceed it. */
function shortTmp(): string {
  const dir = tmpdir();
  return dir.length > 60 ? "/tmp" : dir;
}

/**
 * The worker side: a child-process lookalike whose stdio is a pooled JVM.
 * {@link JvmProcess} drives it exactly as it drives a spawned JVM, except that
 * closing it returns the JVM to the pool instead of shutting it down.
 */
export class PooledChild extends EventEmitter {
  readonly stdout = new PassThrough();
  readonly stderr = new PassThrough();
  readonly stdin: Writable;
  readonly #socket: Socket;
  #buffer = "";
  #exited = false;

  constructor(address: PoolAddress, kind: "host" | "container", slot: string, cache: string = cacheDir()) {
    super();
    const env: Hello["env"] = {};
    for (const name of LAUNCH_ENV) {
      const value = process.env[name];
      if (value !== undefined) env[name] = value;
    }
    const hello: Hello = { type: "hello", token: address.token, kind, cache, slot, env };
    const socket = createConnection(address.path);
    this.#socket = socket;
    socket.setEncoding("utf8");
    socket.setNoDelay(true);
    socket.write(`${JSON.stringify(hello)}\n`);
    this.stdin = new Writable({
      decodeStrings: false,
      write: (chunk: string, _encoding, callback): void => {
        if (socket.destroyed) {
          callback(new Error("rhino-local: pooled runner connection closed"));
          return;
        }
        socket.write(chunk, () => callback());
      },
    });
    socket.on("data", (chunk: string) => this.#onData(chunk));
    socket.on("error", (error) => {
      this.stderr.write(`rhino-local: JVM pool connection failed: ${error.message}\n`);
    });
    socket.on("close", () => this.#exit(null, null));
  }

  #released = false;

  /** Return the JVM to the pool. */
  release(): void {
    this.#released = true;
    this.#socket.end();
  }

  /** Destroy the JVM rather than return it: the pool will start a fresh one for the slot. */
  kill(_signal?: NodeJS.Signals): boolean {
    if (this.#released) return true;
    this.#released = true;
    this.#socket.end(`${JSON.stringify({ type: "rhino-local-pool", kill: true })}\n`);
    return true;
  }

  #onData(chunk: string): void {
    this.#buffer += chunk;
    let nl = this.#buffer.indexOf("\n");
    while (nl !== -1) {
      const line = this.#buffer.slice(0, nl + 1);
      this.#buffer = this.#buffer.slice(nl + 1);
      nl = this.#buffer.indexOf("\n");
      if (!line.startsWith(CONTROL)) {
        this.stdout.write(line);
        continue;
      }
      const frame = JSON.parse(line) as { stderr?: string; exit?: { code: number | null; signal: NodeJS.Signals | null } };
      if (frame.stderr !== undefined) this.stderr.write(frame.stderr);
      if (frame.exit !== undefined) this.#exit(frame.exit.code, frame.exit.signal);
    }
  }

  #exit(code: number | null, signal: NodeJS.Signals | null): void {
    if (this.#exited) return;
    this.#exited = true;
    // Let buffered stderr reach listeners before the exit handler reads it.
    setImmediate(() => this.emit("exit", code, signal));
  }
}
