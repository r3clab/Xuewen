import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import LeaderHint from './LeaderHint.svelte';
import { advanceLeader, cancelLeader } from '$app/keys/leader.svelte';
import { LEADER_CHORDS } from '$app/keys/keymap';

beforeEach(() => cancelLeader());
afterEach(() => cancelLeader());

describe('LeaderHint', () => {
  it('shows nothing until a sequence is in flight', () => {
    render(LeaderHint);
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('spells the pending keys and every chord still reachable', async () => {
    render(LeaderHint);
    advanceLeader(' ');
    const hint = await screen.findByRole('status');
    expect(hint).toHaveTextContent('Space');
    for (const c of LEADER_CHORDS) expect(hint).toHaveTextContent(c.label);
  });

  it('lists only the keys still to come, not the ones already typed', async () => {
    render(LeaderHint);
    advanceLeader(' ');
    const hint = await screen.findByRole('status');
    expect(hint.querySelector('kbd')).toHaveTextContent('f');
  });
});
