/** A catalogue entry, as the model's page shows it. */

export interface Fact {
	label: string;
	value: string;
}

/** Words for the keys the catalogue names; a key it adds later shows under its own name. */
const LABELS: Record<string, string> = {
	context_window: 'Context window',
	max_output_tokens: 'Maximum output',
	json_mode: 'JSON mode',
	cache_read: 'Cached input',
	cache_write: 'Cache write'
};

function label(key: string): string {
	const words = key.replaceAll('_', ' ');
	return LABELS[key] ?? words.charAt(0).toUpperCase() + words.slice(1);
}

function shown(value: unknown): string {
	if (typeof value === 'boolean') return value ? 'yes' : 'no';
	if (typeof value === 'number') return new Intl.NumberFormat('en-GB').format(value);
	if (typeof value === 'string') return value;
	if (Array.isArray(value)) return value.map(shown).join(', ');
	if (typeof value === 'object' && value !== null) {
		const parts = Object.entries(value).map(([key, v]) => `${key}: ${shown(v)}`);
		return parts.join('; ');
	}
	return String(value);
}

/** What the object says, one fact per key it has a value for. Nothing is "no" by being absent. */
export function facts(of: unknown): Fact[] {
	if (typeof of !== 'object' || of === null) return [];
	return Object.entries(of)
		.filter(([, value]) => value !== null && value !== undefined)
		.map(([key, value]) => ({ label: label(key), value: shown(value) }));
}

/** A price per million tokens, in as many digits as it was stated with. */
export function rate(value: number | null | undefined, currency?: string | null): string {
	if (value === null || value === undefined) return '';
	const amount = new Intl.NumberFormat('en-GB', {
		minimumFractionDigits: 2,
		maximumFractionDigits: 6
	}).format(value);
	return currency ? `${amount} ${currency}` : amount;
}

/** The rates of a stored price, one fact each. */
export function rates(price: unknown): Fact[] {
	const p = (price ?? {}) as { currency?: string; rates?: Record<string, number | null> };
	return Object.entries(p.rates ?? {})
		.filter(([, value]) => value !== null && value !== undefined)
		.map(([key, value]) => ({ label: label(key), value: rate(value, p.currency) }));
}

/** The address a price was read from, when the catalogue says. */
export function evidenceUrl(price: unknown): string | null {
	const url = (price as { evidence?: { url?: unknown } } | null | undefined)?.evidence?.url;
	return typeof url === 'string' && /^https?:\/\//.test(url) ? url : null;
}
