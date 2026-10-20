/// Handing a generated file to the browser. Kept apart from the code that
/// produces the bytes so the filename rule can be tested without a PDF engine.

/// The name for an exported copy: `Attention Is All You Need (annotated).pdf`.
/// The suffix goes before the extension so the file still opens as a PDF, and a
/// name that already carries it isn't stacked twice when the user exports again.
export function annotatedFilename(name: string): string {
  const base = (name.trim() || 'paper').replace(/\.pdf$/i, '').trim();
  const stem = base.endsWith('(annotated)') ? base : `${base} (annotated)`;
  return `${sanitizeFilename(stem)}.pdf`;
}

/// Strip what a filesystem won't take. Windows is the strict one — its reserved
/// set is a superset of POSIX's, so obeying it works everywhere.
export function sanitizeFilename(name: string): string {
  const cleaned = name
    // Control characters plus the Windows-reserved punctuation.
    .replace(/[\x00-\x1f<>:"/\\|?*]/g, ' ')
    .replace(/\s+/g, ' ')
    // Windows silently drops a trailing dot or space, which would make the
    // saved name differ from the one we offered.
    .replace(/^[\s.]+|[\s.]+$/g, '');
  return cleaned || 'paper';
}

/// Trigger a download. The object URL is revoked on the next macrotask rather
/// than immediately: Safari reads the href *after* the click handler returns,
/// and revoking synchronously hands it a dead URL.
export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.rel = 'noopener';
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
