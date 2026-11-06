/** What a posted form says, in the types the admin API takes. */

export function text(form: FormData, name: string): string {
	return String(form.get(name) ?? '').trim();
}

/** One entry per line, as a textarea of addresses holds them. */
export function lines(form: FormData, name: string): string[] {
	return text(form, name)
		.split('\n')
		.map((l) => l.trim())
		.filter(Boolean);
}

/** Entries separated by commas or lines, as a field of words holds them. */
export function words(form: FormData, name: string): string[] {
	return text(form, name)
		.split(/[,\n]/)
		.map((w) => w.trim())
		.filter(Boolean);
}

export function checked(form: FormData, name: string): boolean {
	return form.get(name) === 'on';
}

/** The fields that have a value. An optional field of the API is left out, never `undefined`. */
export function given<T extends Record<string, unknown>>(fields: T) {
	return Object.fromEntries(Object.entries(fields).filter(([, value]) => value !== undefined)) as {
		[K in keyof T]?: Exclude<T[K], undefined>;
	};
}

/** A number, or `undefined` for an empty field, so that the server's default applies. */
export function number(form: FormData, name: string): number | undefined {
	const value = text(form, name);
	return value === '' ? undefined : Number(value);
}
