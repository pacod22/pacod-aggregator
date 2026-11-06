import { act, read } from '#lib/server/read.ts';
import type { Actions, PageServerLoad } from './$types';

const SHOWN = 50;

export const load: PageServerLoad = async ({ locals }) => {
	const query = { query: { limit: SHOWN } };
	const [runs, changes] = await Promise.all([
		read(locals.api.GET('/api/v1/runs', { params: query })),
		read(locals.api.GET('/api/v1/changes', { params: query }))
	]);
	return { runs, changes };
};

const SOON = 'It starts within a minute, when a scraper next asks.';

export const actions: Actions = {
	check: ({ locals }) =>
		act(locals.api.POST('/api/v1/runs/check'), `A check is asked for. ${SOON}`),
	scan: ({ locals }) => act(locals.api.POST('/api/v1/runs/scan'), `A scan is asked for. ${SOON}`)
};
