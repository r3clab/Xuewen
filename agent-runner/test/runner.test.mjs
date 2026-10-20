import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const runner = fileURLToPath(new URL('../src/runner.mjs', import.meta.url));

test('--preflight passes with both SDKs installed', () => {
  const r = spawnSync(process.execPath, [runner, '--preflight', 'claude_code', 'codex']);
  assert.equal(r.status, 0, r.stderr.toString());
});

test('a malformed request becomes a protocol error event, not a crash', () => {
  const r = spawnSync(process.execPath, [runner], { input: 'not json{' });
  assert.equal(r.status, 1);
  const events = r.stdout.toString().trim().split('\n').map(JSON.parse);
  assert.equal(events.length, 1);
  assert.equal(events[0].type, 'error');
  assert.match(events[0].message, /JSON/);
});

test('an unknown backend becomes a protocol error event', () => {
  const r = spawnSync(process.execPath, [runner], { input: '{"backend":"nope"}' });
  assert.equal(r.status, 1);
  const events = r.stdout.toString().trim().split('\n').map(JSON.parse);
  assert.equal(events[0].type, 'error');
  assert.match(events[0].message, /unknown backend: nope/);
});
