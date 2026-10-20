/// Tags nest by `/`-separated segments (e.g. `security/fuzzing`): a filter
/// on a parent tag matches its children too (chip highlighting, Task 12).

/// Whether `tagName` is `filter` itself or one of its `filter/...` children.
export const isPrefixMatch = (tagName: string, filter: string): boolean =>
  tagName === filter || tagName.startsWith(filter + '/');
