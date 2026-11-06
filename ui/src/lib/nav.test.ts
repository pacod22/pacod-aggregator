import { describe, expect, it } from 'vitest';
import { isCurrent, may, navFor } from './nav';

describe('nav', () => {
	it('shows a reader every page', () => {
		const hrefs = navFor(['agg.catalog.read', 'agg.data.read']).flatMap((g) =>
			g.items.map((i) => i.href)
		);
		expect(hrefs).toEqual(['/', '/catalogue', '/vendors', '/pages', '/runs', '/settings']);
	});

	it('shows nothing to someone who may only read the catalogue', () => {
		expect(navFor(['agg.catalog.read'])).toEqual([]);
		expect(navFor([])).toEqual([]);
	});

	it('takes the wildcard', () => {
		expect(may(['*'], 'agg.data.manage')).toBe(true);
		expect(may(['agg.data.read'], 'agg.data.manage')).toBe(false);
	});

	it('marks the current page', () => {
		expect(isCurrent('/', '/')).toBe(true);
		expect(isCurrent('/', '/catalogue')).toBe(false);
		expect(isCurrent('/catalogue', '/catalogue/agm_1')).toBe(true);
	});
});
