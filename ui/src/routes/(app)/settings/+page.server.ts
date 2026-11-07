import type { components } from '#lib/api/schema.d.ts';
import { checked, given, number, text, words } from '#lib/form.ts';
import { problemText } from '#lib/server/api.ts';
import { act, read } from '#lib/server/read.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const settings = await read(locals.api.GET('/api/v1/settings'));
	return { settings, gateway: settings.gateway_configured ? await offers(locals.api) : null };
};

/** What the gateway offers, or why it could not be asked: the page shows either, never an error. */
async function offers(api: App.Locals['api']) {
	const { data, error, response } = await api.GET('/api/v1/gateway/models');
	if (response.ok && data) return { models: data.models, problem: null };
	return { models: [], problem: problemText(error, response.status) };
}

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
					// The server refuses a name it does not know.
					mode: (text(form, 'mode') || null) as components['schemas']['Mode'] | null,
					extraction_model: text(form, 'extraction_model') || null,
					gateway_model: text(form, 'gateway_model') || null,
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
