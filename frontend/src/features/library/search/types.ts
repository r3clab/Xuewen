export type StatusFilter = 'all' | 'resolved' | 'needs_review';
export type Sort = 'year_desc' | 'year_asc' | 'added_desc' | 'title' | 'name';

export interface Filters {
  q: string;
  status: StatusFilter;
  sort: Sort;
  project: string;
  tag?: string;
  starred?: boolean;
}

export interface SearchOpts {
  title: boolean;
  authors: boolean;
  abstract: boolean;
  body: boolean;
  /// The reader's own annotation notes.
  notes: boolean;
  keyword: boolean;
  semantic: boolean;
}
