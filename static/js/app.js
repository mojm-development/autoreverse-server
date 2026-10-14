// Small behaviours shared by every page. Delegated listeners, so markup swapped in later
// (htmx) works without re-binding.

const THEME_KEY = 'autoreverse-theme';

function storedTheme() {
	try {
		const stored = localStorage.getItem(THEME_KEY);
		return stored === 'dark' || stored === 'light' ? stored : null;
	} catch {
		return null;
	}
}

function effectiveTheme() {
	return storedTheme() ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
}

function setIcon(button, name) {
	const svg = button.querySelector('svg.icon');
	if (!svg) return;
	svg.dataset.testid = `icon-${name}`;
	svg.querySelector('use')?.setAttribute('href', `#icon-${name}`);
}

// ThemeToggle.svelte: moon while dark, sun while light; the title says where a click goes.
function renderThemeToggles() {
	const theme = effectiveTheme();
	for (const button of document.querySelectorAll('[data-theme-toggle]')) {
		button.title = theme === 'dark' ? 'Zu hellem Thema wechseln' : 'Zu dunklem Thema wechseln';
		setIcon(button, theme === 'dark' ? 'moon' : 'sun');
	}
}

document.addEventListener('click', (event) => {
	const target = event.target instanceof Element ? event.target : null;

	const themeButton = target?.closest('[data-theme-toggle]');
	if (themeButton) {
		const next = effectiveTheme() === 'dark' ? 'light' : 'dark';
		try {
			localStorage.setItem(THEME_KEY, next);
		} catch {
			// No storage: the switch still holds for this page.
		}
		document.documentElement.dataset.theme = next;
		renderThemeToggles();
		return;
	}

	// Password show/hide (login page and every other password field that opts in).
	const passwordButton = target?.closest('[data-password-toggle]');
	if (passwordButton) {
		const input = passwordButton.parentElement?.querySelector('input');
		if (!input) return;
		const show = input.type === 'password';
		input.type = show ? 'text' : 'password';
		passwordButton.setAttribute('aria-label', show ? 'Eingabe verbergen' : 'Eingabe anzeigen');
		setIcon(passwordButton, show ? 'eye-off' : 'eye');
	}
});

document.addEventListener('DOMContentLoaded', renderThemeToggles);
document.addEventListener('htmx:afterSwap', renderThemeToggles);
