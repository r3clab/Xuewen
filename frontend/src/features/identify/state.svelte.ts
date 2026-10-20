import type { Candidate } from '$shared/api/generated/Candidate';
import type { IdentifyBody } from '$shared/api/generated/IdentifyBody';

export const identifyState = $state<{
  open: boolean;
  paperId: string | null;
  input: string;
  busy: boolean;
  candidates: Candidate[];
  selected: Candidate | null;
  /// A direct DOI/arXiv body captured at search time (single fetched record flow).
  direct: IdentifyBody | null;
  /// The paper's identifiers at the time the modal opened, so the modal can
  /// warn when the selected candidate would drop one of them.
  current: { doi: string | null; arxiv_id: string | null } | null;
  error: string | null;
}>({
  open: false,
  paperId: null,
  input: '',
  busy: false,
  candidates: [],
  selected: null,
  direct: null,
  current: null,
  error: null,
});

export function resetIdentifyFields(): void {
  identifyState.input = '';
  identifyState.busy = false;
  identifyState.candidates = [];
  identifyState.selected = null;
  identifyState.direct = null;
  identifyState.current = null;
  identifyState.error = null;
}
