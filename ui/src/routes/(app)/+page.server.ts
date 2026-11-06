import { read } from '#lib/server/read.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	if (locals.apiProblem) return { status: null };
	return { status: await read(locals.api.GET('/api/v1/status')) };
};
