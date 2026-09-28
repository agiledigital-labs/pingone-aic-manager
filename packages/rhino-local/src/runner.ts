import { execFile, spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { randomUUID } from "node:crypto";
import { isDeepStrictEqual, promisify } from "node:util";
import {
  cacheDir,
  checkEnvironment,
  containerLaunch,
  ensureRhinoJar,
  ensureRunnerClasses,
  hostLaunch,
  laneFromEnv,
  parseEnvironment,
  type JvmLane,
  type Launch,
  type RunnerEnvironment,
} from "./jvm.ts";
import { parseJobResponse, type JobRequest, type JobResponse } from "./protocol.ts";

const execFileAsync = promisify(execFile);

const READY = "rhino-local-runner ready";

export class RhinoRunnerExitError extends Error {
  readonly exitCode: number | null;
  readonly signal: NodeJS.Signals | null;
  readonly stderr: string;

  constructor(exitCode: number | null, signal: NodeJS.Signals | null, stderr: string) {
    super(
      `rhino-local: JVM runner exited (code=${exitCode}, signal=${signal})` +
        (stderr.trim() ? `: ${stderr.trim()}` : "")
    );
    this.name = "RhinoRunnerExitError";
    this.exitCode = exitCode;
    this.signal = signal;
    this.stderr = stderr;
  }
}

/**
 * The host JVM and the AM image's JVM answered one job differently. This is
 * the whole reason the `both` lane exists, so it fails the test rather than
 * picking a winner.
 */
export class LaneDivergenceError extends Error {
  readonly job: string;
  readonly host: JobResponse;
  readonly container: JobResponse;

  constructor(job: string, host: JobResponse, container: JobResponse) {
    super(
      `rhino-local: host and container JVMs disagree on ${job}\n` +
        `  host:      ${JSON.stringify(withoutId(host))}\n` +
        `  container: ${JSON.stringify(withoutId(container))}`
    );
    this.name = "LaneDivergenceError";
    this.job = job;
    this.host = host;
    this.container = container;
  }
}

function withoutId(response: JobResponse): Omit<JobResponse, "id"> {
  const { id: _id, ...rest } = response;
  return rest;
}

export interface SpawnRunnerOptions {
  /** Default: `AIC_SCRIPT_TESTER_JVM`, else `host`. */
  lane?: JvmLane;
  /** Jar and compiled-class cache. Default: `AIC_SCRIPT_TESTER_CACHE`, else `~/.cache/aic-script-tester`. */
  cache?: string;
  spawnTimeoutMs?: number;
}

type Pending = {
  resolve: (response: JobResponse) => void;
  reject: (error: Error) => void;
};

/** One JVM speaking the line protocol. {@link RhinoRunner} holds one or two. */
class JvmProcess {
  readonly lane: "host" | "container";
  readonly containerName: string | undefined;
  #child: ChildProcessWithoutNullStreams;
  #pending = new Map<string, Pending>();
  #stdoutBuf = "";
  #stderrBuf = "";
  #closed = false;
  #exit: { code: number | null; signal: NodeJS.Signals | null } | null = null;
  #exitWaiters: Array<() => void> = [];
  environment: RunnerEnvironment | undefined;

  constructor(lane: "host" | "container", launch: Launch) {
    this.lane = lane;
    this.containerName = launch.containerName;
    const child = spawn(launch.command, launch.args, {
      env: launch.env,
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.#child = child;
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => this.#onStdout(chunk));
    child.stderr.on("data", (chunk: string) => {
      this.#stderrBuf += chunk;
    });
    child.stdin.on("error", () => {
      // EPIPE after a crash is reported via the exit handler.
    });
    child.on("error", (error) => {
      this.#stderrBuf += `\n${error.message}`;
    });
    child.on("exit", (code, signal) => {
      this.#exit = { code, signal };
      const error = new RhinoRunnerExitError(code, signal, this.#stderrBuf);
      for (const [id, pending] of this.#pending) {
        this.#pending.delete(id);
        pending.reject(error);
      }
      for (const waiter of this.#exitWaiters) {
        waiter();
      }
      this.#exitWaiters = [];
    });
  }

  get stderr(): string {
    return this.#stderrBuf;
  }

  async waitUntilReady(timeoutMs: number): Promise<void> {
    const started = Date.now();
    for (;;) {
      const at = this.#stderrBuf.indexOf(READY);
      const end = at === -1 ? -1 : this.#stderrBuf.indexOf("\n", at);
      if (end !== -1) {
        const env = parseEnvironment(this.#stderrBuf.slice(at, end));
        checkEnvironment(env, this.lane);
        this.environment = env;
        return;
      }
      if (this.#exit) {
        throw new RhinoRunnerExitError(this.#exit.code, this.#exit.signal, this.#stderrBuf);
      }
      if (Date.now() - started > timeoutMs) {
        await this.kill();
        throw new Error(
          `rhino-local: ${this.lane} runner did not become ready within ${timeoutMs}ms: ${this.#stderrBuf}`
        );
      }
      await sleep(25);
    }
  }

  send(id: string, payload: Record<string, unknown>): Promise<JobResponse> {
    if (this.#closed || this.#exit) {
      return Promise.reject(
        this.#exit
          ? new RhinoRunnerExitError(this.#exit.code, this.#exit.signal, this.#stderrBuf)
          : new Error("rhino-local: runner is closed")
      );
    }
    return new Promise<JobResponse>((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
      const ok = this.#child.stdin.write(`${JSON.stringify(payload)}\n`);
      if (!ok) {
        this.#child.stdin.once("error", reject);
      }
    });
  }

  async close(): Promise<void> {
    if (this.#exit) {
      await this.#removeContainer();
      return;
    }
    if (this.#closed) {
      await this.#waitForExit();
      await this.#removeContainer();
      return;
    }
    this.#closed = true;
    try {
      this.#child.stdin.end();
    } catch {
      // already closed
    }
    const exited = await Promise.race([
      this.#waitForExit().then(() => true),
      sleep(5_000).then(() => false),
    ]);
    if (!exited) {
      this.#child.kill("SIGTERM");
      const exitedAfterTerm = await Promise.race([
        this.#waitForExit().then(() => true),
        sleep(2_000).then(() => false),
      ]);
      if (!exitedAfterTerm) {
        this.#child.kill("SIGKILL");
        await this.#waitForExit();
      }
    }
    for (const [id, pending] of this.#pending) {
      this.#pending.delete(id);
      pending.reject(new Error("rhino-local: runner closed with job still pending"));
    }
    await this.#removeContainer();
  }

  async kill(): Promise<void> {
    this.#closed = true;
    this.#child.kill("SIGKILL");
    await Promise.race([this.#waitForExit(), sleep(2_000)]);
    await this.#removeContainer();
  }

  #onStdout(chunk: string): void {
    this.#stdoutBuf += chunk;
    let nl = this.#stdoutBuf.indexOf("\n");
    while (nl !== -1) {
      const line = this.#stdoutBuf.slice(0, nl);
      this.#stdoutBuf = this.#stdoutBuf.slice(nl + 1);
      if (line.length > 0) {
        this.#dispatch(line);
      }
      nl = this.#stdoutBuf.indexOf("\n");
    }
  }

  #dispatch(line: string): void {
    let parsed: unknown;
    try {
      parsed = JSON.parse(line) as unknown;
    } catch (error) {
      const first = this.#pending.entries().next().value;
      if (first) {
        this.#pending.delete(first[0]);
        first[1].reject(
          new Error(`rhino-local: unparseable runner stdout: ${line} (${String(error)})`)
        );
      }
      return;
    }
    let response: JobResponse;
    try {
      response = parseJobResponse(parsed, line);
    } catch (error) {
      const first = this.#pending.entries().next().value;
      if (first) {
        this.#pending.delete(first[0]);
        first[1].reject(error instanceof Error ? error : new Error(String(error)));
      }
      return;
    }
    const pending = this.#pending.get(response.id);
    if (!pending) {
      return;
    }
    this.#pending.delete(response.id);
    pending.resolve(response);
  }

  #waitForExit(): Promise<void> {
    if (this.#exit) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      this.#exitWaiters.push(resolve);
    });
  }

  async #removeContainer(): Promise<void> {
    if (this.containerName === undefined) {
      return;
    }
    try {
      await execFileAsync("docker", ["rm", "-f", this.containerName], { timeout: 10_000 });
    } catch {
      // already removed via --rm, or never created
    }
  }
}

export class RhinoRunner {
  readonly lane: JvmLane;
  #primary: JvmProcess;
  #shadow: JvmProcess | undefined;

  private constructor(lane: JvmLane, primary: JvmProcess, shadow: JvmProcess | undefined) {
    this.lane = lane;
    this.#primary = primary;
    this.#shadow = shadow;
  }

  static async spawn(options: SpawnRunnerOptions = {}): Promise<RhinoRunner> {
    const lane = options.lane ?? laneFromEnv();
    const cache = options.cache ?? cacheDir();
    const jar = await ensureRhinoJar(cache);
    const classes = await ensureRunnerClasses(jar, { cache });
    const spawnTimeoutMs = options.spawnTimeoutMs ?? 60_000;
    const processes: JvmProcess[] = [];
    try {
      if (lane === "host" || lane === "both") {
        processes.push(new JvmProcess("host", hostLaunch(classes, jar)));
      }
      if (lane === "container" || lane === "both") {
        processes.push(new JvmProcess("container", containerLaunch(classes, jar)));
      }
      await Promise.all(processes.map((p) => p.waitUntilReady(spawnTimeoutMs)));
    } catch (error) {
      await Promise.all(processes.map((p) => p.kill()));
      throw error;
    }
    const [primary, shadow] = processes;
    if (primary === undefined) {
      throw new Error("rhino-local: no runner lane selected");
    }
    return new RhinoRunner(lane, primary, shadow);
  }

  /** What the (primary) JVM reported at startup. */
  get environment(): RunnerEnvironment {
    const env = this.#primary.environment;
    if (env === undefined) {
      throw new Error("rhino-local: runner environment read before ready");
    }
    return env;
  }

  async eval(job: JobRequest): Promise<JobResponse> {
    const id = job.id ?? randomUUID();
    const payload: Record<string, unknown> = { id, source: job.source };
    if (job.sourceName !== undefined) {
      payload.sourceName = job.sourceName;
    }
    if (job.languageVersion !== undefined) {
      payload.languageVersion = job.languageVersion;
    }
    if (job.timeoutMs !== undefined) {
      payload.timeoutMs = job.timeoutMs;
    }
    if (job.globals !== undefined) {
      payload.globals = job.globals;
    }
    if (job.preamble !== undefined) {
      payload.preamble = job.preamble;
    }
    if (job.preambleName !== undefined) {
      payload.preambleName = job.preambleName;
    }
    if (job.classAllowList !== undefined) {
      payload.classAllowList = job.classAllowList;
    }
    if (job.resultGlobal !== undefined) {
      payload.resultGlobal = job.resultGlobal;
    }
    if (this.#shadow === undefined) {
      return this.#primary.send(id, payload);
    }
    const [host, container] = await Promise.all([
      this.#primary.send(id, payload),
      this.#shadow.send(id, payload),
    ]);
    if (!isDeepStrictEqual(withoutId(host), withoutId(container))) {
      throw new LaneDivergenceError(job.sourceName ?? id, host, container);
    }
    return host;
  }

  async close(): Promise<void> {
    await Promise.all(this.#processes().map((p) => p.close()));
  }

  /** Forcibly stop every JVM. Every pending eval rejects. Used by crash tests. */
  async kill(): Promise<void> {
    await Promise.all(this.#processes().map((p) => p.kill()));
  }

  get stderr(): string {
    return this.#processes()
      .map((p) => p.stderr)
      .join("");
  }

  #processes(): JvmProcess[] {
    return this.#shadow === undefined ? [this.#primary] : [this.#primary, this.#shadow];
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}
