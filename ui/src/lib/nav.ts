/** The sidebar: one list, filtered by what the signed-in person may do. */

export interface NavItem {
	href: string;
	label: string;
	/** The aggregator permission the page needs. */
	needs: string;
}

export const NAV: { label: string; items: NavItem[] }[] = [
	{
		label: 'Data',
		items: [
			{ href: '/', label: 'Overview', needs: 'agg.data.read' },
			{ href: '/catalogue', label: 'Catalogue', needs: 'agg.data.read' }
		]
	},
	{
		label: 'Sources',
		items: [
			{ href: '/vendors', label: 'Vendors', needs: 'agg.data.read' },
			{ href: '/pages', label: 'Pages', needs: 'agg.data.read' }
		]
	},
	{
		label: 'Operation',
		items: [
			{ href: '/runs', label: 'Runs', needs: 'agg.data.read' },
			{ href: '/settings', label: 'Settings', needs: 'agg.data.read' }
		]
	}
];

/** Whether a person with these permissions holds `permission`. */
export function may(permissions: string[], permission: string): boolean {
	return permissions.includes('*') || permissions.includes(permission);
}

export function navFor(permissions: string[]) {
	return NAV.map((g) => ({ ...g, items: g.items.filter((i) => may(permissions, i.needs)) })).filter(
		(g) => g.items.length > 0
	);
}

export function isCurrent(href: string, path: string): boolean {
	return href === '/' ? path === '/' : path === href || path.startsWith(href + '/');
}
