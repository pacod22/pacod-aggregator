import { describe, expect, it } from 'vitest';
import { checked, given, lines, number, text, words } from './form';

const form = (fields: Record<string, string>) => {
	const data = new FormData();
	for (const [name, value] of Object.entries(fields)) data.set(name, value);
	return data;
};

describe('form', () => {
	it('trims text and takes a missing field as empty', () => {
		expect(text(form({ a: '  b ' }), 'a')).toBe('b');
		expect(text(form({}), 'a')).toBe('');
	});

	it('splits lines and words, without empty entries', () => {
		expect(lines(form({ a: 'https://a\n\n  https://b \n' }), 'a')).toEqual([
			'https://a',
			'https://b'
		]);
		expect(words(form({ a: 'pricing, models\nrates,' }), 'a')).toEqual([
			'pricing',
			'models',
			'rates'
		]);
		expect(lines(form({}), 'a')).toEqual([]);
	});

	it('takes a ticked box as true and a missing one as false', () => {
		expect(checked(form({ a: 'on' }), 'a')).toBe(true);
		expect(checked(form({}), 'a')).toBe(false);
	});

	it('leaves out the fields without a value, and keeps those that are false or zero', () => {
		expect(given({ a: undefined, b: 0, c: false, d: '', e: null })).toEqual({
			b: 0,
			c: false,
			d: '',
			e: null
		});
	});

	it('leaves an empty number to the server', () => {
		expect(number(form({ a: '12.5' }), 'a')).toBe(12.5);
		expect(number(form({ a: '0' }), 'a')).toBe(0);
		expect(number(form({ a: ' ' }), 'a')).toBeUndefined();
	});
});
