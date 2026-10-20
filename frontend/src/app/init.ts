import { initAnnotationTools } from '$features/reader/annotations/state.svelte';
import { loadChatModels } from '$features/reader/ask/chat.svelte';
import { initColumnWidths } from '$features/library/table/columnWidths.svelte';
import { loadPapers, loadStats } from '$features/library/actions';
import { loadProjects, loadTags } from '$features/library/organization/actions';
import { loadSearchStatus } from '$features/library/search/actions';
import { initTabs } from '$features/reader/tabs/state.svelte';
import { initPdfAppearance, initTheme } from '$shared/ui/theme.svelte';
import { initDock } from '$features/reader/dock/state.svelte';
import { loadSettings } from '$shared/api/settings.svelte';

export function initApp(): void {
  initTheme();
  initDock();
  initPdfAppearance();
  initColumnWidths();
  initAnnotationTools();
  void initTabs();
  loadStats();
  loadProjects();
  // Here, not with the filter pills: a session restored onto a PDF tab never
  // mounts them, and the tag editor's autocomplete reads this store.
  loadTags();
  loadPapers();
  loadSearchStatus();
  void loadSettings();
  loadChatModels();
}
