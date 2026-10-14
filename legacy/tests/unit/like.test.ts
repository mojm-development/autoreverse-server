import { describe, it, expect } from 'vitest';
import { escapeLike } from '../../src/lib/server/library/like';

describe('escapeLike', () => {
	it('escapes backslash first, then % and _', () => {
		expect(escapeLike('a\\b%c_d')).toBe('a\\\\b\\%c\\_d');
	});
	it('leaves plain text untouched', () => {
		expect(escapeLike('billion')).toBe('billion');
	});
});
