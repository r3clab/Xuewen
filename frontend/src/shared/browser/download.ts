/// Hand a generated file to the browser. The object URL is revoked on the
/// next macrotask rather than immediately: Safari reads the href *after* the
/// click handler returns, and revoking synchronously hands it a dead URL.
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
