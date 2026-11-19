/// `{@attach closeOnScroll(close)}`: close a floating surface as soon as
/// anything under it scrolls, since its anchor just moved. Capture phase is
/// load-bearing: the app scrolls in inner overflow panes (the library table,
/// the PDF stage) whose scroll events don't bubble, so a bubble-phase window
/// listener would never hear the scrolls that actually move the anchor.
export function closeOnScroll(close: () => void): () => () => void {
  return () => {
    const onscroll = () => close();
    window.addEventListener('scroll', onscroll, { capture: true, passive: true });
    return () => window.removeEventListener('scroll', onscroll, { capture: true });
  };
}
