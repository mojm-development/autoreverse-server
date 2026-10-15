// Page changes without a reload (the SvelteKit client router in legacy): htmx boosts links and
// forms, and this module points every boosted response at `#page` only, so the player outside it
// keeps playing. A response that is not an app page (the login page after a logout, an error
// page) is opened as a normal navigation instead.

const PAGE = '#page';

/** Whether a server response is a page of the player layout. */
export function isAppPage(html) {
	return typeof html === 'string' && /<div[^>]*\bid="page"/.test(html);
}

export function installNavigation({ onPageChange }) {
	document.addEventListener('htmx:beforeSwap', (event) => {
		const detail = event.detail;
		if (!detail?.boosted) return;
		if (!isAppPage(detail.serverResponse)) {
			detail.shouldSwap = false;
			const target = detail.xhr?.responseURL || detail.pathInfo?.finalRequestPath;
			if (target) window.location.assign(target);
			return;
		}
		const page = document.querySelector(PAGE);
		if (!page) return;
		// Error pages of the app layout (a form re-rendered with 4xx) are still pages to show.
		detail.shouldSwap = true;
		detail.isError = false;
		detail.target = page;
		detail.selectOverride = PAGE;
		detail.swapOverride = 'outerHTML show:window:top';
	});
	document.addEventListener('htmx:afterSettle', (event) => {
		if (event.detail?.boosted || event.target?.id === 'page') onPageChange();
	});
	document.addEventListener('htmx:historyRestore', () => onPageChange());
}
