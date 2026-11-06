import { checked, given, lines, text } from '#lib/form.ts';
import { act, read } from '#lib/server/read.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => ({
	vendors: await read(locals.api.GET('/api/v1/vendors'))
});

export const actions: Actions = {
	add: async ({ locals, request }) => {
		const form = await request.formData();
		const name = text(form, 'display_name');
		return act(
			locals.api.POST('/api/v1/vendors', {
				body: {
					id: text(form, 'id'),
					display_name: name,
					...given({ currency: text(form, 'currency') || undefined }),
					sites: lines(form, 'sites'),
					enabled: true,
					complete: checked(form, 'complete')
				}
			}),
			`${name} is added. The next scan looks for its pages.`
		);
	}
};
