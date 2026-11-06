import { identity } from '#lib/server/identity.ts';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ locals }) => ({
	principal: locals.principal,
	apiProblem: locals.apiProblem,
	account: identity?.account ?? null
});
