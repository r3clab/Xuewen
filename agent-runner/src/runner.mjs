// Per-turn agent runner. stdin: one JSON request. stdout: JSON-lines events
// (delta | tool | done | error). Spawned by Xuewen's AgentService; exits
// when the turn ends. See docs/superpowers/specs/2026-07-16-*-design.md.
// Backends load lazily so `--preflight` (and a turn for one backend) never
// needs the other backend's SDK installed.
import { readFileSync } from 'node:fs';
import { emit } from './protocol.mjs';

// `--preflight <backend>...`: verify the Node version and import each named
// backend, without touching stdin (a normal turn blocks reading it). Import
// loads the SDK's JS only — it never execs the vendored CLI binary, so a
// broken binary still surfaces per-turn, not here. Exits 0/1 with stderr.
if (process.argv[2] === '--preflight') {
  const problems = [];
  const major = Number(process.versions.node.split('.')[0]);
  if (!(major >= 20)) {
    problems.push(`Node ${process.versions.node} is too old — the agent needs Node >= 20`);
  }
  for (const backend of process.argv.slice(3)) {
    try {
      if (backend === 'claude_code') await import('./claude.mjs');
      else if (backend === 'codex') await import('./codex.mjs');
    } catch (e) {
      problems.push(
        `the ${backend} backend failed to load (run \`npm --prefix agent-runner install\`?): ${String(e?.message ?? e)}`,
      );
    }
  }
  if (problems.length > 0) {
    process.stderr.write(problems.join('\n') + '\n');
    process.exit(1);
  }
  process.exit(0);
}

let req;
try {
  // Inside the try so even a malformed request becomes an error event, not
  // an unhandled exception with a raw stack.
  req = JSON.parse(readFileSync(0, 'utf8'));
  if (req.backend === 'claude_code') await (await import('./claude.mjs')).runClaude(req);
  else if (req.backend === 'codex') await (await import('./codex.mjs')).runCodex(req);
  else throw new Error(`unknown backend: ${req.backend}`);
  emit({ type: 'done' });
} catch (e) {
  emit({ type: 'error', message: String(e?.message ?? e) });
  process.exitCode = 1;
}
