const KEY = 'autoreverse-theme';

export function getTheme(): 'dark' | 'light' | null {
	if (typeof localStorage === 'undefined') return null;
	const stored = localStorage.getItem(KEY);
	return stored === 'dark' || stored === 'light' ? stored : null;
}

export function setTheme(theme: 'dark' | 'light'): void {
	localStorage.setItem(KEY, theme);
	document.documentElement.dataset.theme = theme;
}

export function toggleTheme(): 'dark' | 'light' {
	const current =
		getTheme() ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
	const next = current === 'dark' ? 'light' : 'dark';
	setTheme(next);
	return next;
}
