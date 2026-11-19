/// `href` when it is a web URL, else null. Every link built from data the
/// app did not write itself — a paper's stored url (identify round-trips an
/// arbitrary candidate's), LLM-parsed references, a PDF's own /URI actions —
/// passes through here: a javascript:/data: href would run in the app origin.
export function safeHttpUrl(href: string): string | null {
  try {
    const { protocol } = new URL(href);
    return protocol === 'http:' || protocol === 'https:' ? href : null;
  } catch {
    return null;
  }
}
