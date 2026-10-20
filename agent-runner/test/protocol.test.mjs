import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { composePrompt, emit, toolDetail } from '../src/protocol.mjs';

test('emit serializes each event type exactly as the shared fixture', () => {
  // fixtures/events.jsonl is the wire contract's canonical form, asserted
  // from both sides: here against `emit`, and in src/agent/mod.rs against
  // the Rust AgentEvent deserializer — change fixture and both together.
  const lines = readFileSync(new URL('./fixtures/events.jsonl', import.meta.url), 'utf8')
    .trim()
    .split('\n');
  // One entry per event type, shaped exactly like the real emit call sites
  // (claude.mjs / codex.mjs / runner.mjs).
  const events = [
    { type: 'delta', text: 'Hel' },
    { type: 'tool', name: 'Read', detail: 'paper.txt' },
    { type: 'done' },
    { type: 'error', message: 'boom' },
  ];
  const out = [];
  const original = process.stdout.write;
  process.stdout.write = (s) => {
    out.push(s);
    return true;
  };
  try {
    for (const ev of events) emit(ev);
  } finally {
    process.stdout.write = original;
  }
  assert.deepEqual(
    out.map((s) => s.trimEnd()),
    lines,
  );
});

test('composePrompt lists workspace contents, paper, transcript, question', () => {
  const p = composePrompt({
    hasRepo: true,
    paper: { title: 'Attention Is All You Need', venue: 'NeurIPS', year: 2017, authors: ['Vaswani'] },
    transcript: [
      { role: 'user', content: 'hi' },
      { role: 'assistant', content: 'hello' },
    ],
    question: 'where is the mask applied?',
  });
  assert.match(p, /paper\.txt/);
  assert.match(p, /repo\//);
  assert.match(p, /Attention Is All You Need/);
  assert.match(p, /Authors: Vaswani/);
  assert.match(p, /Researcher: hi/);
  assert.match(p, /You \(earlier\): hello/);
  assert.match(p, /Researcher: where is the mask applied\?$/);
});

test('composePrompt omits the repo line when none is attached', () => {
  const p = composePrompt({ hasRepo: false, paper: { title: 'T' }, transcript: [], question: 'q' });
  assert.doesNotMatch(p, /repo\//);
});

test('toolDetail picks a representative input field and truncates', () => {
  assert.equal(toolDetail({ file_path: 'paper.txt' }), 'paper.txt');
  assert.equal(toolDetail({ pattern: 'mask' }), 'mask');
  assert.equal(toolDetail(null), '');
  assert.equal(toolDetail({ file_path: 'x'.repeat(200) }).length, 120);
});
