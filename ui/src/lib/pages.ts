// Copied from pacod-gw (commit 50fe4b7, ui/src/lib/price-pages.ts), for vendors in place of providers.
/** The pages the aggregator reads, as the Pages page shows them. */

import type { components } from '#lib/api/schema.d.ts';

type Page = components['schemas']['PageView'];

/** The pages of each vendor, vendors by name; within one, the server's order (priority). */
export function pageGroups(pages: Page[]): { vendor_id: string; vendor: string; pages: Page[] }[] {
	const groups = new Map<string, { vendor_id: string; vendor: string; pages: Page[] }>();
	for (const page of pages) {
		const group = groups.get(page.vendor_id) ?? {
			vendor_id: page.vendor_id,
			// The server sends no name for a vendor that was deleted since.
			vendor: page.vendor || page.vendor_id,
			pages: []
		};
		group.pages.push(page);
		groups.set(page.vendor_id, group);
	}
	return [...groups.values()].sort((a, b) =>
		a.vendor.localeCompare(b.vendor, 'en', { numeric: true })
	);
}

/** What the last check found, and what the scan said of the page, in a line. */
export function pageNote(page: Page): string {
	const s = page.state;
	const notes: string[] = [];
	if (page.status === 'proposed' && page.score != null) notes.push(`scored ${page.score}`);
	if (page.moved_from) notes.push(`replaces ${page.moved_from}`);
	if (s?.flag === 'gone') notes.push('gone: it answers 404 or 410');
	if (s?.flag === 'moved') notes.push(`moved to ${s.moved_to ?? 'another address'}`);
	if (s?.outcome) {
		const outcome = s.outcome.replaceAll('_', ' ');
		notes.push(s.message ? `${outcome}: ${s.message}` : outcome);
	}
	if (s && s.consecutive_failures > 1) notes.push(`${s.consecutive_failures} failures in a row`);
	if (s?.check_requested) notes.push('to be checked at the next run');
	return notes.join('; ');
}
