import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import DockComposer from './DockComposer.svelte';
import { chat } from './chat.svelte';

beforeEach(() => {
  chat.available = true;
  chat.models = [{ id: '0', label: 'Mock A' }, { id: '1', label: 'Mock B' }];
  chat.modelId = '0';
  chat.paperId = 'p1';
  chat.messages = [];
  chat.pending = null;
  chat.streaming = null;
  chat.busy = false;
  chat.error = null;
  chat.draft = '';
  localStorage.clear();
  vi.unstubAllGlobals();
  vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
});

describe('DockComposer', () => {
  it('offers the model picker and the ask box', () => {
    render(DockComposer);
    expect(screen.getByLabelText('Model')).toHaveValue('0');
    expect(screen.getByPlaceholderText('Ask about this paper…')).toBeInTheDocument();
  });

  it('changing the model persists the choice', async () => {
    render(DockComposer);
    await userEvent.selectOptions(screen.getByLabelText('Model'), '1');
    expect(localStorage.getItem('xuewen-chat-model')).toBe('1');
  });

  it('offers Clear only once there is a conversation to clear', async () => {
    render(DockComposer);
    expect(screen.queryByRole('button', { name: 'Clear conversation' })).not.toBeInTheDocument();
  });

  it('clear asks for confirmation before deleting', async () => {
    chat.messages = [
      { id: 1, role: 'user', content: 'q', model: null, created_at: '', tools: null },
      { id: 2, role: 'assistant', content: 'a', model: 'Mock A', created_at: '', tools: null },
    ];
    const fetchSpy = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchSpy);
    render(DockComposer);
    await userEvent.click(screen.getByRole('button', { name: 'Clear conversation' }));
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(screen.getByText('Clear this conversation?')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(fetchSpy).toHaveBeenCalled();
  });
});
