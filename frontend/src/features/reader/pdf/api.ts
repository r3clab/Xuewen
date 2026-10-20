export function pdfUrl(id: string): string {
  return `/papers/${encodeURIComponent(id)}/pdf`;
}
