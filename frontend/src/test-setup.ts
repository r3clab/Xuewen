import '@testing-library/jest-dom/vitest';

// jsdom implements no layout, no rendering and none of the platform pieces
// below. Each stub stands in for the browser so production code never has to
// ask whether it is running under a test. Suites that opt into the node
// environment have no DOM to stub.
if (typeof window !== 'undefined') stubBrowser();

function stubBrowser(): void {
  // No Web Animations API: Svelte's `animate:` directive (flip, crossfade)
  // calls Element.getAnimations() to avoid clobbering an in-flight animation.
  Element.prototype.getAnimations ??= () => [];

  // No scrollIntoView at all; components that keep a highlighted row in view
  // would throw on render.
  Element.prototype.scrollIntoView ??= () => {};

  // No matchMedia. Every query answers "no" except reduced motion, which the
  // tests ask for: time-based transitions would leave elements lingering in
  // the DOM and make assertions flaky. svelte/motion reads it at module load.
  window.matchMedia ??= (query: string) =>
    ({
      matches: query.includes('prefers-reduced-motion: reduce'),
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }) as MediaQueryList;

  // No ResizeObserver; with no layout nothing would ever resize anyway.
  globalThis.ResizeObserver ??= class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  };

  // No pointer capture.
  Element.prototype.setPointerCapture ??= () => {};
  Element.prototype.releasePointerCapture ??= () => {};
  Element.prototype.hasPointerCapture ??= () => false;

  // No canvas 2D context: text measures a fixed 7px per character.
  HTMLCanvasElement.prototype.getContext = function (kind: string) {
    if (kind !== '2d') return null;
    return { font: '', measureText: (text: string) => ({ width: text.length * 7 }) };
  } as typeof HTMLCanvasElement.prototype.getContext;

  // No modal dialogs: jsdom has <dialog> but neither showModal() nor close().
  // This keeps the parts the app relies on — `open`, focus moving into the
  // dialog (its [autofocus] element, else its first focusable) and back to
  // the opener on close, and the `close` event.
  const openers = new WeakMap<HTMLDialogElement, Element | null>();
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    openers.set(this, document.activeElement);
    this.open = true;
    const target =
      this.querySelector<HTMLElement>('[autofocus]') ??
      this.querySelector<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
    target?.focus();
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    if (!this.open) return;
    this.open = false;
    const opener = openers.get(this);
    if (opener instanceof HTMLElement) opener.focus();
    this.dispatchEvent(new Event('close'));
  };
}
