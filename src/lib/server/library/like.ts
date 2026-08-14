/** Order matters: backslash first, then the two LIKE wildcards — mirrors
 * capstan/src/capstan/library/like.py::escape_like exactly. Returns the raw
 * escaped substring only; callers wrap with `%...%` themselves. */
export function escapeLike(text: string): string {
	return text.replaceAll('\\', '\\\\').replaceAll('%', '\\%').replaceAll('_', '\\_');
}

export function likePattern(q: string): string {
	return `%${escapeLike(q)}%`;
}
