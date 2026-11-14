// Runs before anything is painted: a stored choice (see app.js) takes effect immediately,
// otherwise it stays with prefers-color-scheme (see tokens.css) — without this early script there
// would be a brief flash of the wrong theme on load. A file of its own rather than inline, so the
// Content-Security-Policy needs no inline scripts.
(function () {
	try {
		var stored = localStorage.getItem('autoreverse-theme');
		if (stored === 'light' || stored === 'dark') {
			document.documentElement.setAttribute('data-theme', stored);
		}
	} catch (error) {
		// e.g. private mode with no localStorage — falls back to the system theme.
	}
})();
