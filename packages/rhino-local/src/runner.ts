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
import {
  parseHostCall,
  parseJobResponse,
  type HostCall,
  type HostError,
  type HostReply,
  type JobRequest,
  type JobResponse,
  type JsonValue,
} from "./protocol.ts";
export type { HostCall, HostError, JobRequest, JobResponse, JsonValue } from "./protocol.ts";

export interface EvalOptions {
  onCall?: (call: HostCall) => JsonValue | Promise<JsonValue>;
}

/** Throw from onCall to expose a CREST-shaped failure to the IDM script. */
export class HostCallError extends Error {
  readonly class: string;
  readonly code: number;
  readonly detail: JsonValue;
  constructor(error: HostError) {
    super(error.message);
    this.name = "HostCallError";
    this.class = error.class;
    this.code = error.code;
    this.detail = error.detail;
  }
}

function hostError(error: unknown): HostError {
  const record =
    typeof error === "object" && error !== null ? (error as Record<string, unknown>) : {};
  const normalized: HostError = {
    class: typeof record.class === "string" ? record.class : "rhino.local.HostCallException",
    message: typeof record.message === "string" ? record.message : String(error),
    code: typeof record.code === "number" ? record.code : 500,
    detail: (record.detail ?? {}) as JsonValue,
  };
  try {
    return JSON.parse(JSON.stringify(normalized)) as HostError;
  } catch {
    return {
      class: "rhino.local.HostCallException",
      message: "rhino-local: host error is not JSON serializable",
      code: 500,
      detail: {},
    };
  }
}

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

  constructor(job: string, host: JobResponse, container: JobResponse, detail?: string) {
    super(
      `rhino-local: host and container JVMs disagree on ${job}\n` +
        `  host:      ${JSON.stringify(withoutId(host))}\n` +
        `  container: ${JSON.stringify(withoutId(container))}` +
        (detail === undefined ? "" : `\n  host calls: ${detail}`)
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
  onCall: ((call: HostCall) => Promise<HostReply>) | undefined;
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
  #spawnError: Error | undefined;
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
      this.#spawnError = error;
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
      if (this.#spawnError) {
        const remedy =
          this.lane === "host"
            ? " Set AIC_SCRIPT_TESTER_JAVA_HOME or JAVA_HOME to a Java 25 installation, or put java on PATH."
            : "";
        throw new Error(
          `rhino-local: could not start ${this.lane} runner: ${this.#spawnError.message}.${remedy}`,
          { cause: this.#spawnError }
        );
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

  send(
    id: string,
    payload: Record<string, unknown>,
    onCall?: Pending["onCall"]
  ): Promise<JobResponse> {
    if (this.#closed || this.#exit) {
      return Promise.reject(
        this.#exit
          ? new RhinoRunnerExitError(this.#exit.code, this.#exit.signal, this.#stderrBuf)
          : new Error("rhino-local: runner is closed")
      );
    }
    if (this.#pending.has(id)) {
      return Promise.reject(new Error(`rhino-local: duplicate active job id: ${id}`));
    }
    return new Promise<JobResponse>((resolve, reject) => {
      this.#pending.set(id, { resolve, reject, onCall });
      const ok = this.#child.stdin.write(`${JSON.stringify(payload)}\n`);
      if (!ok) {
        const onError = (error: Error): void => {
          this.#child.stdin.off("drain", onDrain);
          this.#pending.delete(id);
          reject(error);
        };
        const onDrain = (): void => {
          this.#child.stdin.off("error", onError);
        };
        this.#child.stdin.once("error", onError);
        this.#child.stdin.once("drain", onDrain);
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
    if (this.#spawnError) {
      await this.#removeContainer();
      return;
    }
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
      const call = parseHostCall(parsed);
      if (call !== undefined) {
        void this.#answerCall(call);
        return;
      }
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

  async #answerCall(call: HostCall): Promise<void> {
    const pending = this.#pending.get(call.id);
    if (pending === undefined) return;
    let reply: HostReply;
    try {
      if (pending.onCall === undefined)
        throw new Error("rhino-local: no onCall handler for IDM job");
      reply = await pending.onCall(call);
      // Validate serializability before writing, so a bad handler cannot strand a worker.
      const line = JSON.stringify(reply);
      if (this.#pending.get(call.id) === pending && !this.#closed && !this.#exit)
        this.#child.stdin.write(`${line}\n`);
      return;
    } catch (error) {
      reply = {
        type: "reply",
        id: call.id,
        seq: call.seq,
        error: hostError(error),
      };
    }
    if (this.#pending.get(call.id) === pending && !this.#closed && !this.#exit) {
      this.#child.stdin.write(`${JSON.stringify(reply)}\n`);
    }
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
      await execFileAsync("docker", ["rm", "-f", this.containerName], {
        timeout: 10_000,
      });
    } catch {
      // already removed via --rm, or never created
    }
  }
}

export class RhinoRunner {
  readonly lane: JvmLane;
  #primary: JvmProcess;
  #shadow: JvmProcess | undefined;
  #activeIds = new Set<string>();

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

  async eval(job: JobRequest, options: EvalOptions = {}): Promise<JobResponse> {
    const id = job.id ?? randomUUID();
    if (this.#activeIds.has(id)) throw new Error(`rhino-local: duplicate active job id: ${id}`);
    this.#activeIds.add(id);
    try {
      return await this.#evalJob({ ...job, id }, options);
    } finally {
      this.#activeIds.delete(id);
    }
  }

  async #evalJob(job: JobRequest & { id: string }, options: EvalOptions): Promise<JobResponse> {
    const id = job.id;
    const payload: Record<string, unknown> = { id, source: job.source };
    if (job.engine !== undefined) payload.engine = job.engine;
    if (job.bindings !== undefined) payload.bindings = job.bindings;
    if (job.resourceNameBindings !== undefined)
      payload.resourceNameBindings = job.resourceNameBindings;
    if (job.reportMissing !== undefined) payload.reportMissing = job.reportMissing;
    if (job.crestBindings !== undefined) payload.crestBindings = job.crestBindings;
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
    const transcript: Array<{ call: HostCall; reply: HostReply | undefined }> = [];
    const primaryCall = async (call: HostCall): Promise<HostReply> => {
      const entry = {
        call: structuredClone(call),
        reply: undefined as HostReply | undefined,
      };
      transcript.push(entry);
      let reply: HostReply;
      try {
        if (options.onCall === undefined)
          throw new Error("rhino-local: no onCall handler for IDM job");
        const result = await options.onCall(call);
        if (result === undefined)
          throw new Error("rhino-local: onCall must return a JSON value (use null)");
        // Freeze the reply before handing control back to user code or the shadow.
        reply = JSON.parse(
          JSON.stringify({ type: "reply", id, seq: entry.call.seq, result })
        ) as HostReply;
      } catch (error) {
        reply = {
          type: "reply",
          id,
          seq: entry.call.seq,
          error: hostError(error),
        };
      }
      entry.reply = reply;
      return reply;
    };
    if (this.#shadow === undefined) return this.#primary.send(id, payload, primaryCall);
    if (job.engine !== "idm") {
      const [host, container] = await Promise.all([
        this.#primary.send(id, payload),
        this.#shadow.send(id, payload),
      ]);
      if (!isDeepStrictEqual(withoutId(host), withoutId(container))) {
        throw new LaneDivergenceError(job.sourceName ?? id, host, container);
      }
      return host;
    }
    // Sequential shadow replay avoids duplicate effects and naturally supports nested evals.
    const host = await this.#primary.send(id, payload, primaryCall);
    let at = 0;
    let divergence: string | undefined;
    const container = await this.#shadow.send(id, payload, async (call) => {
      const recorded = transcript[at++];
      if (recorded === undefined || !isDeepStrictEqual(recorded.call, call)) {
        divergence ??= `expected ${JSON.stringify(recorded?.call)}, received ${JSON.stringify(call)}`;
        return {
          type: "reply",
          id,
          seq: call.seq,
          error: {
            class: "rhino.local.LaneDivergenceException",
            message: "rhino-local: shadow host call sequence differs",
            code: 500,
            detail: {},
          },
        };
      }
      // Primary finished without a reply only when its worker timed out waiting.
      // Leave the shadow waiting too, so its own deadline produces the same outcome.
      return recorded.reply ?? new Promise<HostReply>(() => {});
    });
    if (divergence !== undefined || at !== transcript.length) {
      throw new LaneDivergenceError(
        job.sourceName ?? id,
        host,
        container,
        divergence ?? `expected ${transcript.length} calls, received ${at}`
      );
    }
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
