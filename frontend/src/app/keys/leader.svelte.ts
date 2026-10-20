import { exactChord, matchingChords, type ChordBinding } from './keymap';

/// How long a half-finished sequence waits for its next key. Helix's own
/// default; long enough to read the hint, short enough that a forgotten Space
/// doesn't silently swallow the next keystroke.
export const LEADER_TIMEOUT_MS = 1000;

/// The keys pressed so far in an unfinished chord. Empty means no sequence is
/// in flight; anything else is what `LeaderHint` renders.
export const leader = $state<{ pending: string[] }>({ pending: [] });

let timer: ReturnType<typeof setTimeout> | null = null;

function reset(): void {
  leader.pending = [];
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
}

/// The continuations still reachable from what has been typed — the hint's
/// content, and the reason an unbound key can cancel immediately instead of
/// waiting out the timeout.
export function leaderContinuations(): readonly ChordBinding[] {
  return matchingChords(leader.pending);
}

/// Feed one key into the pending sequence. Returns whether the key was
/// consumed: a key that continues or completes a chord belongs to the leader
/// and must not also reach its own binding (Space then `j` opens no picker,
/// but must not move the library selection either).
export function advanceLeader(key: string): boolean {
  const next = [...leader.pending, key];
  const chord = exactChord(next);
  if (chord) {
    reset();
    // A gated chord swallows its key rather than falling through: the
    // sequence was still addressed to the leader, it just had nothing to do.
    if (!chord.when || chord.when()) chord.run();
    return true;
  }
  if (matchingChords(next).length === 0) {
    // Nothing can follow this. Cancel now rather than making the user wait
    // out the timeout, and report it consumed so a mistyped second key does
    // not also fire whatever it is bound to on its own.
    const wasPending = leader.pending.length > 0;
    reset();
    return wasPending;
  }
  leader.pending = next;
  // Restarted per keystroke, not measured from the first press, so a long
  // sequence is not racing a deadline set before it began.
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(reset, LEADER_TIMEOUT_MS);
  return true;
}

/// Abandon a half-typed sequence (Esc, or the dispatcher standing aside).
/// Returns whether anything was pending, so Esc can stop at this rung.
export function cancelLeader(): boolean {
  const wasPending = leader.pending.length > 0;
  reset();
  return wasPending;
}
