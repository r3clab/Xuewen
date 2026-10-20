import { Codex } from '@openai/codex-sdk';
import { composePrompt, emit } from './protocol.mjs';

/// Codex backend: the OS-enforced read-only sandbox may run read-only
/// commands but cannot write or reach the network. agent_message items grow
/// across item.updated events, so emit only the unseen suffix per item id.
export async function runCodex(req) {
  const codex = new Codex();
  const thread = codex.startThread({
    workingDirectory: req.workspace,
    sandboxMode: 'read-only',
    skipGitRepoCheck: true,
    ...(req.model ? { model: req.model } : {}),
  });
  const { events } = await thread.runStreamed(composePrompt(req));
  const sent = new Map(); // agent_message id -> chars already emitted
  for await (const ev of events) {
    if ((ev.type === 'item.updated' || ev.type === 'item.completed') && ev.item.type === 'agent_message') {
      const prev = sent.get(ev.item.id) ?? 0;
      const text = ev.item.text ?? '';
      if (text.length > prev) {
        emit({ type: 'delta', text: text.slice(prev) });
        sent.set(ev.item.id, text.length);
      }
    } else if (ev.type === 'item.started' && ev.item.type === 'command_execution') {
      emit({ type: 'tool', name: 'run', detail: String(ev.item.command ?? '').slice(0, 120) });
    } else if (ev.type === 'turn.failed') {
      throw new Error(ev.error?.message ?? 'the turn failed');
    } else if (ev.type === 'error') {
      throw new Error(ev.message);
    }
  }
}
