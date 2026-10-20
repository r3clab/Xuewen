/// The one style for a paper's manual "known as" name (`papers.name`) wherever
/// it appears as a chip: the Details dock.
///
/// Amber because the name is how you actually refer to the paper — "the RVSpec
/// paper" — so it must not read as fine print.
///
/// Mono is what keeps it apart from the tag chips, which are the same amber
/// whenever a tag filter is active (`chipClasses` in PaperRowTags.svelte).
/// Identity and topic can't be told apart by color alone, so: mono = the thing
/// the paper proposes, sans = what it's about.
export const NAME_CHIP =
  'rounded border border-amber-700/40 bg-amber-700/10 px-1.5 py-px font-mono text-chip font-semibold text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/15 dark:text-amber-400';
