import {
  execFile,
  spawn,
  type ChildProcessWithoutNullStreams,
} from "node:child_process";
import { randomUUID } from "node:crypto";
import { promisify } from "node:util";
import { parseJobResponse, type JobRequest, type JobResponse } from "./protocol.ts";
import { repoRoot, runRunnerScript } from "./paths.ts";

const execFileAsync = promisify(execFile);

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

export interface SpawnRunnerOptions {
  repoRoot?: string;
  /** Override the launcher argv. Default: `run-runner.sh`. */
  command?: string[];
  spawnTimeoutMs?: number;
}

type Pending = {
  resolve: (response: JobResponse) => void;
  reject: (error: Error) => void;
};

export class RhinoRunner {
  readonly containerName: string;
  #child: ChildProcessWithoutNullStreams;
  #pending = new Map<string, Pending>();
  #stdoutBuf = "";
  #stderrBuf = "";
  #closed = false;
  #exit: { code: number | null; signal: NodeJS.Signals | null } | null = null;
  #exitWaiters: Array<() => void> = [];

  private constructor(child: ChildProcessWithoutNullStreams, containerName: string) {
    this.#child = child;
    this.containerName = containerName;
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

  static async spawn(options: SpawnRunnerOptions = {}): Promise<RhinoRunner> {
    const containerName = `rhino-local-runner-${randomUUID()}`;
    const cwd = options.repoRoot ?? repoRoot;
    const command = options.command ?? [runRunnerScript];
    const [argv0, ...argv] = command;
    if (!argv0) {
      throw new Error("rhino-local: spawn command is empty");
    }
    const child = spawn(argv0, argv, {
      cwd,
      env: { ...process.env, RHINO_LOCAL_CONTAINER: containerName },
      stdio: ["pipe", "pipe", "pipe"],
    });
    const runner = new RhinoRunner(child, containerName);
    const spawnTimeoutMs = options.spawnTimeoutMs ?? 60_000;
    await runner.#waitUntilReady(spawnTimeoutMs);
    return runner;
  }

  eval(job: JobRequest): Promise<JobResponse> {
    if (this.#closed || this.#exit) {
      return Promise.reject(
        this.#exit
          ? new RhinoRunnerExitError(this.#exit.code, this.#exit.signal, this.#stderrBuf)
          : new Error("rhino-local: runner is closed")
      );
    }
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

  /** Forcibly stop the JVM. Every pending eval rejects. Used by crash tests. */
  async kill(): Promise<void> {
    this.#closed = true;
    this.#child.kill("SIGKILL");
    await Promise.race([this.#waitForExit(), sleep(2_000)]);
    await this.#removeContainer();
  }

  get stderr(): string {
    return this.#stderrBuf;
  }

  async #waitUntilReady(timeoutMs: number): Promise<void> {
    const started = Date.now();
    while (!this.#stderrBuf.includes("rhino-local-runner ready")) {
      if (this.#exit) {
        throw new RhinoRunnerExitError(this.#exit.code, this.#exit.signal, this.#stderrBuf);
      }
      if (Date.now() - started > timeoutMs) {
        await this.kill();
        throw new Error(
          `rhino-local: runner did not become ready within ${timeoutMs}ms: ${this.#stderrBuf}`
        );
      }
      await sleep(25);
    }
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
    try {
      await execFileAsync("docker", ["rm", "-f", this.containerName], { timeout: 10_000 });
    } catch {
      // already removed via --rm, or never created
    }
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}
