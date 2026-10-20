/// One JSON-lines event to the Rust parent.
export function emit(ev) {
  process.stdout.write(JSON.stringify(ev) + '\n');
}

/// One prompt string carrying instructions, paper identity, prior turns, and
/// the new question — uniform across backends (neither needs a separate
/// system prompt for our use; the paper text itself stays on disk and is
/// read on demand).
export function composePrompt(req) {
  const p = req.paper ?? {};
  const lines = [
    'You are a research assistant answering questions about one specific paper.',
    'Your working directory contains:',
    "  - paper.txt — the paper's full extracted text.",
    req.hasRepo ? "  - repo/ — the paper's code repository (read-only)." : null,
    'Read and search these files to ground every answer; when neither the paper nor the code contains the answer, say so plainly.',
    'Answer in plain prose without markdown formatting.',
    '',
    `Paper: ${p.title ?? '(untitled)'}${p.year ? ` (${p.venue ? p.venue + ' ' : ''}${p.year})` : ''}`,
    p.authors?.length ? `Authors: ${p.authors.join(', ')}` : null,
    '',
  ].filter((l) => l !== null);
  for (const turn of req.transcript ?? []) {
    lines.push(`${turn.role === 'user' ? 'Researcher' : 'You (earlier)'}: ${turn.content}`, '');
  }
  lines.push(`Researcher: ${req.question}`);
  return lines.join('\n');
}

/// A short human-readable detail for a tool call's activity chip.
export function toolDetail(input) {
  if (input == null || typeof input !== 'object') return '';
  const v = input.file_path ?? input.pattern ?? input.path ?? '';
  return String(v).slice(0, 120);
}
