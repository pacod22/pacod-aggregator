import { describe, expect, it } from 'vitest';
import type { components } from '#lib/api/schema.d.ts';
import { pageGroups, pageNote } from './pages';

type Page = components['schemas']['PageView'];

const page = (over: Partial<Page>): Page =>
	({
		id: 'agp_1',
		created_at: '2026-11-05T00:00:00Z',
		updated_at: '2026-11-05T00:00:00Z',
		vendor_id: 'deepseek',
		vendor: 'DeepSeek',
		url: 'https://api-docs.deepseek.com/quick_start/pricing',
		kind: 'page',
		format: '',
		status: 'active',
		found_by: 'preset',
		priority: 0,
		...over
	}) as Page;

describe('pageGroups', () => {
	it('groups the pages by vendor, vendors by name', () => {
		const groups = pageGroups([
			page({ id: 'a', vendor_id: 'zai', vendor: 'Z.ai' }),
			page({ id: 'b' }),
			page({ id: 'c', vendor_id: 'zai', vendor: 'Z.ai' }),
			page({ id: 'd', vendor_id: 'gone', vendor: '' })
		]);
		expect(groups.map((g) => [g.vendor, g.pages.map((p) => p.id)])).toEqual([
			['DeepSeek', ['b']],
			['gone', ['d']],
			['Z.ai', ['a', 'c']]
		]);
	});
});

describe('pageNote', () => {
	it('says what the last check and the scan found', () => {
		expect(pageNote(page({}))).toBe('');
		const state = {
			outcome: 'failed',
			message: 'the page answered 403',
			consecutive_failures: 3,
			check_requested: true,
			flag: 'moved',
			moved_to: 'https://x.example/new'
		} as Page['state'];
		expect(pageNote(page({ state }))).toBe(
			'moved to https://x.example/new; failed: the page answered 403; 3 failures in a row; to be checked at the next run'
		);
		expect(
			pageNote(page({ status: 'proposed', score: 24, moved_from: 'https://x.example/old' }))
		).toBe('scored 24; replaces https://x.example/old');
	});
});
