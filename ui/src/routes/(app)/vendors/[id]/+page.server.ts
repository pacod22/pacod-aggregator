import { redirect } from '@sveltejs/kit';
import { checked, given, lines, text } from '#lib/form.ts';
import { act, read } from '#lib/server/read.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const path = { path: { id: params.id } };
	const query = { query: { vendor_id: params.id } };
	const [vendor, pages] = await Promise.all([
		read(locals.api.GET('/api/v1/vendors/{id}', { params: path })),
		read(locals.api.GET('/api/v1/pages', { params: query }))
	]);
	return { vendor, pages };
};

export const actions: Actions = {
	save: async ({ locals, params, request }) => {
		const form = await request.formData();
		return act(
			locals.api.PUT('/api/v1/vendors/{id}', {
				params: { path: { id: params.id } },
				body: {
					id: params.id,
					display_name: text(form, 'display_name'),
					...given({ currency: text(form, 'currency') || undefined }),
					sites: lines(form, 'sites'),
					enabled: checked(form, 'enabled'),
					complete: checked(form, 'complete')
				}
			}),
			'The vendor is saved.'
		);
	},
	remove: async ({ locals, params }) => {
		const result = await act(
			locals.api.DELETE('/api/v1/vendors/{id}', { params: { path: { id: params.id } } }),
			'removed'
		);
		if ('done' in result) redirect(303, '/vendors');
		return result;
	}
};
