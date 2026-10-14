export function escapeLike(text: string): string {
	return text.replaceAll('\\', '\\\\').replaceAll('%', '\\%').replaceAll('_', '\\_');
}

export function likePattern(q: string): string {
	return `%${escapeLike(q)}%`;
}
