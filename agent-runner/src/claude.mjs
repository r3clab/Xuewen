import { query } from '@anthropic-ai/claude-agent-sdk';
import { composePrompt, emit, toolDetail } from './protocol.mjs';

/// Claude Code backend: read-only tools, no Bash => no execution and no
/// network. Text streams via partial-message deltas; tool chips come from
/// the completed assistant messages (their input is only complete there).
export async function runClaude(req) {
  for await (const msg of query({
    prompt: composePrompt(req),
    options: {
      cwd: req.workspace,
      ...(req.model ? { model: req.model } : {}),
      allowedTools: ['Read', 'Grep', 'Glob'],
      disallowedTools: ['Bash', 'Write', 'Edit', 'NotebookEdit', 'WebFetch', 'WebSearch', 'Agent', 'TodoWrite'],
      permissionMode: 'default',
      includePartialMessages: true,
      maxTurns: 50,
    },
  })) {
    if (msg.type === 'stream_event') {
      const ev = msg.event;
      if (ev.type === 'content_block_delta' && ev.delta?.type === 'text_delta') {
        emit({ type: 'delta', text: ev.delta.text });
      }
    } else if (msg.type === 'assistant') {
      for (const block of msg.message?.content ?? []) {
        if (block.type === 'tool_use') {
          emit({ type: 'tool', name: block.name, detail: toolDetail(block.input) });
        }
      }
    } else if (msg.type === 'result' && msg.subtype !== 'success') {
      throw new Error(`the agent stopped early: ${msg.subtype}`);
    }
  }
}
