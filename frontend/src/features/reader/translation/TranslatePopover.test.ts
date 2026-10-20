import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import TranslatePopover from './TranslatePopover.svelte';
import { translateBox, closeTranslate } from './state.svelte';
import { appSettings } from '$shared/api/settings.svelte';
import { dock } from '$features/reader/dock/state.svelte';
import { chat } from '$features/reader/ask/chat.svelte';

beforeEach(() => {
  closeTranslate();
  appSettings.translate = { enabled: true, providers: ['llm', 'deepl'], default_provider: 'llm', target_lang: 'zh', trigger: 'auto' };
  dock.open = false;
  dock.entry = null;
  chat.draft = '';
  chat.available = true;
  vi.unstubAllGlobals();
  vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn(async () => {}) } });
});

function openBox() {
  translateBox.open = true;
  translateBox.source = 'hello world';
  translateBox.translation = '你好世界';
  translateBox.sourceLang = null;
  translateBox.provider = 'llm';
  translateBox.loading = false;
  translateBox.error = null;
  translateBox.x = 100;
  translateBox.y = 100;
}

describe('TranslatePopover', () => {
  it('renders source and translation when open', () => {
    openBox();
    render(TranslatePopover);
    expect(screen.getByText(/hello world/)).toBeInTheDocument();
    expect(screen.getByText('你好世界')).toBeInTheDocument();
  });

  it('Copy writes the translation to the clipboard', async () => {
    openBox();
    render(TranslatePopover);
    await userEvent.click(screen.getByRole('button', { name: /copy/i }));
    expect((navigator.clipboard.writeText as ReturnType<typeof vi.fn>)).toHaveBeenCalledWith('你好世界');
  });

  it('Ask about this prefills the chat draft and opens the dock on the composer', async () => {
    openBox();
    render(TranslatePopover);
    await userEvent.click(screen.getByRole('button', { name: /ask about this/i }));
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('ask');
    expect(chat.draft).toContain('hello world');
  });

  it('renders nothing when closed', () => {
    closeTranslate();
    render(TranslatePopover);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows the detected source language in the direction chip', () => {
    openBox();
    translateBox.sourceLang = 'EN';
    render(TranslatePopover);
    expect(screen.getByText('EN → zh')).toBeInTheDocument();
  });
});
