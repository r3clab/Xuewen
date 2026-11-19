import type { Spring } from 'svelte/motion';

/// Shared motion vocabulary. Every animated surface derives its timing from
/// these tokens so the whole UI moves with one accent — never hardcode a
/// duration in a component.
export const DUR = { fast: 150, base: 250, slow: 400 } as const;

/// The one standard ease-out (quart). Mirrors --ease-fluent in app.css.
export const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)';

/// Presets for `new Spring(value, SPRINGS.x)` from svelte/motion.
export const SPRINGS = {
  pane: { stiffness: 0.18, damping: 0.85 },
} as const;

/// Whether motion should complete instantly (the accessibility preference).
export function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/// Resolve a duration. 0 under reduced motion.
export function dur(ms: number): number {
  return prefersReducedMotion() ? 0 : ms;
}

/// Drive a Spring toward `target`, snapping instead of animating under
/// reduced motion — the one home for that branch, so every spring surface
/// honors the same policy.
export function springTo(spring: Spring<number>, target: number): void {
  if (prefersReducedMotion()) {
    void spring.set(target, { instant: true });
  } else {
    spring.target = target;
  }
}
