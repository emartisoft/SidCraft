import { existsSync } from "node:fs";
import { delimiter, join } from "node:path";

export interface RunOptions {
  cwd?: string;
  env?: Record<string, string | undefined>;
}

/**
 * Runs an external program without a shell (no `bash -c`, no command string
 * parsing). Arguments are passed as an explicit argv array.
 */
export async function run(
  command: string,
  args: string[],
  options: RunOptions = {},
): Promise<void> {
  const proc = Bun.spawn([command, ...args], {
    cwd: options.cwd,
    env: { ...process.env, ...options.env },
    stdin: "ignore",
    stdout: "inherit",
    stderr: "inherit",
  });
  const code = await proc.exited;
  if (code !== 0) {
    throw new Error(`${command} ${args.join(" ")} exited with code ${code}`);
  }
}

/** Runs a program and reports success instead of throwing. */
export async function runQuietly(
  command: string,
  args: string[],
  options: RunOptions = {},
): Promise<boolean> {
  try {
    const proc = Bun.spawn([command, ...args], {
      cwd: options.cwd,
      env: { ...process.env, ...options.env },
      stdin: "ignore",
      stdout: "ignore",
      stderr: "ignore",
    });
    return (await proc.exited) === 0;
  } catch {
    return false;
  }
}

export interface CaptureResult {
  code: number;
  stdout: string;
  stderr: string;
}

/**
 * Runs a program and captures its output. Used for `gsettings get`, where the
 * current value has to be read before it can be written back.
 */
export async function capture(
  command: string,
  args: string[],
  options: RunOptions = {},
): Promise<CaptureResult> {
  try {
    const proc = Bun.spawn([command, ...args], {
      cwd: options.cwd,
      env: { ...process.env, ...options.env },
      stdin: "ignore",
      stdout: "pipe",
      stderr: "pipe",
    });
    const [stdout, stderr, code] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
      proc.exited,
    ]);
    return { code, stdout, stderr };
  } catch (error) {
    return {
      code: -1,
      stdout: "",
      stderr: error instanceof Error ? error.message : String(error),
    };
  }
}

/** Looks up a program on PATH without invoking an external `which`. */
export function which(command: string): string | undefined {
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    if (!dir) continue;
    const candidate = join(dir, command);
    if (existsSync(candidate)) return candidate;
  }
  return undefined;
}
