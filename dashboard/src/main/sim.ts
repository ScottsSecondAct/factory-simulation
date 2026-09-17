import { ChildProcess, execFile, spawn } from "child_process";
import { existsSync } from "fs";
import { join, resolve } from "path";
import { createInterface, Interface } from "readline";
import { EventEmitter } from "events";

export interface SimSpawnOpts {
  duration?: number;
  snapshotInterval?: number;
  millMtbf?: number;
  agvMtbf?: number;
  seed?: number;
  noFaults?: boolean;
}

export class SimManager extends EventEmitter {
  private proc: ChildProcess | null = null;
  private rl: Interface | null = null;
  private stderrBuffer: string[] = [];

  findBinary(): string {
    const root = resolve(__dirname, "../..");
    const candidates = [
      join(root, "..", "target", "release", "factory-sim.exe"),
      join(root, "..", "target", "debug", "factory-sim.exe"),
      join(root, "..", "target", "release", "factory-sim"),
      join(root, "..", "target", "debug", "factory-sim"),
    ];
    for (const p of candidates) {
      if (existsSync(p)) return p;
    }
    throw new Error(
      "factory-sim binary not found. Run `cargo build` in the project root."
    );
  }

  start(opts: SimSpawnOpts = {}): void {
    if (this.proc) this.kill();

    const bin = this.findBinary();
    const args = ["--ipc"];
    if (opts.duration != null) args.push("--duration", String(opts.duration));
    if (opts.snapshotInterval != null)
      args.push("--snapshot-interval", String(opts.snapshotInterval));
    if (opts.millMtbf != null) args.push("--mill-mtbf", String(opts.millMtbf));
    if (opts.agvMtbf != null) args.push("--agv-mtbf", String(opts.agvMtbf));
    if (opts.seed != null) args.push("--seed", String(opts.seed));
    if (opts.noFaults) args.push("--no-faults");

    this.proc = spawn(bin, args, { stdio: ["pipe", "pipe", "pipe"] });
    this.stderrBuffer = [];

    this.rl = createInterface({ input: this.proc.stdout! });
    this.rl.on("line", (line: string) => {
      const trimmed = line.trim();
      if (!trimmed) return;
      try {
        const msg = JSON.parse(trimmed);
        this.emit("message", msg);
      } catch {
        this.emit("stderr", `[WARN] unparseable: ${trimmed}`);
      }
    });

    this.proc.stderr!.on("data", (chunk: Buffer) => {
      const lines = chunk.toString().split("\n");
      for (const line of lines) {
        const t = line.trim();
        if (t) {
          this.stderrBuffer.push(t);
          if (this.stderrBuffer.length > 500) this.stderrBuffer.shift();
          this.emit("stderr", t);
        }
      }
    });

    this.proc.on("exit", (code: number | null) => {
      this.emit("exit", code, this.stderrBuffer.slice(-20));
      this.proc = null;
      this.rl = null;
    });

    this.proc.on("error", (err: Error) => {
      this.emit("error", err.message);
    });
  }

  send(cmd: Record<string, unknown>): void {
    if (!this.proc?.stdin?.writable) return;
    this.proc.stdin.write(JSON.stringify(cmd) + "\n");
  }

  kill(): void {
    if (this.proc) {
      this.proc.kill();
      this.proc = null;
      this.rl = null;
    }
  }

  get running(): boolean {
    return this.proc !== null;
  }
}
