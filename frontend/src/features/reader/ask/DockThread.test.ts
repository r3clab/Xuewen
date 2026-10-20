import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import DockThread from './DockThread.svelte';
import { chat } from './chat.svelte';

beforeEach(() => {
  chat.available = true;
  chat.paperId = 'p1';
  chat.messages = [];
  chat.pending = null;
  chat.streaming = null;
  chat.streamTools = [];
  chat.busy = false;
  chat.error = null;
});

describe('DockThread', () => {
  it('renders nothing until there is a conversation — the dock is then the record alone', () => {
    const { container } = render(DockThread);
    expect(container.textContent?.trim()).toBe('');
  });

  it('renders the model label under assistant turns', () => {
    chat.messages = [
      { id: 1, role: 'user', content: 'q', model: null, created_at: '', tools: null },
      { id: 2, role: 'assistant', content: 'a', model: 'Mock A', created_at: '', tools: null },
    ];
    render(DockThread);
    expect(screen.getByText('Mock A', { selector: 'p' })).toBeInTheDocument();
  });

  it('renders tool chips above the assistant text', () => {
    chat.messages = [
      { id: 1, role: 'user', content: 'q', model: null, created_at: '', tools: null },
      {
        id: 2,
        role: 'assistant',
        content: 'a',
        model: 'Claude Code',
        created_at: '',
        tools: [{ name: 'Read', detail: 'paper.txt' }],
      },
    ];
    render(DockThread);
    expect(screen.getByText(/Read paper\.txt/)).toBeInTheDocument();
  });

  it('surfaces a stream error', () => {
    chat.error = 'agent runner failed';
    render(DockThread);
    expect(screen.getByText('agent runner failed')).toBeInTheDocument();
  });
});
