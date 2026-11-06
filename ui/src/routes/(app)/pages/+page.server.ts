import { given, number, text } from '#lib/form.ts';
import { act, read } from '#lib/server/read.ts';
import type { Actions, PageServerLoad, RequestEvent } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const status = url.searchParams.get('status') || undefined;
	const vendor_id = url.searchParams.get('vendor_id') || undefined;
	const [pages, vendors] = await Promise.all([
		read(locals.api.GET('/api/v1/pages', { params: { query: { status, vendor_id } } })),
		read(locals.api.GET('/api/v1/vendors'))
	]);
	return { pages, vendors, status, vendor_id };
};

/** A decision on one page: confirm, dismiss or check it. */
async function decide(event: RequestEvent, what: 'confirm' | 'dismiss' | 'check', said: string) {
	const form = await event.request.formData();
	const params = { path: { id: text(form, 'id') } };
	const api = event.locals.api;
	const call = {
		confirm: () => api.POST('/api/v1/pages/{id}/confirm', { params }),
		dismiss: () => api.POST('/api/v1/pages/{id}/dismiss', { params }),
		check: () => api.POST('/api/v1/pages/{id}/check', { params })
	}[what];
	return act(call(), `${text(form, 'url')} ${said}`);
}

export const actions: Actions = {
	confirm: (event) =>
		decide(event, 'confirm', 'is read from the next check on, which starts within a minute.'),
	dismiss: (event) =>
		decide(event, 'dismiss', 'is not read, and the scan does not propose it again.'),
	check: (event) =>
		decide(event, 'check', 'is read at the next check, which starts within a minute.'),
	remove: async ({ locals, request }) => {
		const form = await request.formData();
		return act(
			locals.api.DELETE('/api/v1/pages/{id}', {
				params: { path: { id: text(form, 'id') } }
			}),
			`${text(form, 'url')} is no longer read.`
		);
	},
	add: async ({ locals, request }) => {
		const form = await request.formData();
		const url = text(form, 'url');
		return act(
			locals.api.POST('/api/v1/pages', {
				body: {
					vendor_id: text(form, 'vendor_id'),
					url,
					...given({
						kind: text(form, 'kind') || undefined,
						format: text(form, 'format') || undefined,
						priority: number(form, 'priority'),
						notes: text(form, 'notes') || undefined
					})
				}
			}),
			`${url} is added, and read at the next check.`
		);
	}
};
