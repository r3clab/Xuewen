import '@testing-library/jest-dom/vitest';

// jsdom has no Web Animations API. Svelte's `animate:` directive (flip,
// crossfade) calls Element.getAnimations() to avoid clobbering an in-flight
// animation; stub it so components using those directives don't throw.
if (typeof Element !== 'undefined' && !Element.prototype.getAnimations) {
  Element.prototype.getAnimations = () => [];
}

// jsdom implements no layout, so it ships no Element.scrollIntoView at all;
// components that keep a highlighted row in view would throw on render.
if (typeof Element !== 'undefined' && !Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
