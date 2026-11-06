import { checked, given, number, text, words } from '#lib/form.ts';
import { act, read } from '#lib/server/read.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => ({
	settings: await read(locals.api.GET('/api/v1/settings'))
});

export const actions: Actions = {
	// The version the form was read at: someone else's change in between is refused, not undone.
	save: async ({ locals, request }) => {
		const form = await request.formData();
		return act(
			locals.api.PUT('/api/v1/settings', {
				body: {
					version: Number(text(form, 'version')),
					enabled: checked(form, 'enabled'),
					schedule: text(form, 'schedule'),
					scan_schedule: text(form, 'scan_schedule'),
					time_zone: text(form, 'time_zone'),
					extraction_model: text(form, 'extraction_model') || null,
					...given({
						extraction_context_window: number(form, 'extraction_context_window'),
						max_extractions_per_run: number(form, 'max_extractions_per_run')
					}),
					scan: {
						...given({
							max_pages_per_vendor: number(form, 'max_pages_per_vendor'),
							min_score: number(form, 'min_score')
						}),
						keywords: words(form, 'keywords')
					},
					openrouter_price_vendors: words(form, 'openrouter_price_vendors')
				}
			}),
			'The settings are saved.'
		);
	}
};
