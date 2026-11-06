import { read } from '#lib/server/read.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => ({
	model: await read(locals.api.GET('/api/v1/models/{id}', { params: { path: { id: params.id } } }))
});
