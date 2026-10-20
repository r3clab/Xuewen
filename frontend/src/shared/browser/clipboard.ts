/// Copy text to the clipboard. Uses the async Clipboard API when available
/// (secure contexts: https or localhost), and otherwise falls back to the
/// legacy execCommand path — which is what makes copy work when the UI is served
/// over plain HTTP to a non-localhost host, where `navigator.clipboard` is
/// undefined. Throws if neither path succeeds.
export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.setAttribute('readonly', '');
  ta.style.position = 'fixed';
  ta.style.top = '0';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  try {
    if (!document.execCommand('copy')) {
      throw new Error('copy command was rejected');
    }
  } finally {
    document.body.removeChild(ta);
  }
}
