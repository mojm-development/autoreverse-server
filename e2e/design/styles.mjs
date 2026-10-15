// Static style comparison of the legacy SvelteKit UI and the Rust UI — no browser. For every page
// it fetches the HTML from both running servers (e2e/design/compare.sh with DESIGN_SETUP_ONLY=1
// starts them with the same data), loads the page's stylesheets, minifies both sides with the
// same lightningcss settings (so values are spelled alike), computes the cascaded value of every
// property on every visible element (selector matching via css-select, specificity, source
// order, !important, inline styles, media queries for the chosen width and theme, inheritance
// for inherited properties), aligns the two element trees and prints what differs — with the
// winning selector on each side. Hover/focus rules are compared as states of their own.
//
//   cd e2e/design && npm install && node styles.mjs [page …]
//   WIDTHS=1440,390 THEMES=dark node styles.mjs book-detail --unmatched
//   DESIGN_USER=NonAdmin node styles.mjs favorites   # empty states, no admin controls
//
// It finds what scoped Svelte styles hid: a single-class rule losing to app.css's
// `button:not(…)`, a page rule reaching another page's element (all page CSS is loaded on every
// page here), a component rule reaching a nested component. What it cannot see — layout, fonts,
// JS-driven state — the screenshots of compare.spec.ts cover.
import { mkdirSync, writeFileSync } from 'node:fs';
import { parseDocument, DomUtils } from 'htmlparser2';
import { selectAll } from 'css-select';
import { transform } from 'lightningcss';
import { calculate } from 'specificity';

const PORT = { legacy: Number(process.env.LEGACY_PORT ?? 3200), rust: Number(process.env.RUST_PORT ?? 3100) };
const OUT = new URL('./out/styles/', import.meta.url);
const args = process.argv.slice(2);
const flags = new Set(args.filter((a) => a.startsWith('--')));
const only = args.filter((a) => !a.startsWith('--'));
const WHY = true;
const STATES_FLAG = !flags.has('--no-states');
let WIDTH = 1440;
let theme = 'dark';

const token = {};
async function login(server) {
	const res = await fetch(`http://127.0.0.1:${PORT[server]}/auth/login`, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({ name: process.env.DESIGN_USER ?? 'Oliver', password: 'hunter2hunter2' })
	});
	token[server] = (await res.json()).token;
}
async function fetchText(server, path, auth = true) {
	const res = await fetch(`http://127.0.0.1:${PORT[server]}${path}`, {
		headers: auth ? { cookie: `autoreverse_session=${token[server]}` } : {}
	});
	return res.text();
}
async function itemId(server, title) {
	const { items } = JSON.parse(await fetchText(server, `/items?q=${encodeURIComponent(title)}`));
	return items.find((i) => i.title === title).id;
}
async function playlistId(server) {
	const { playlists } = JSON.parse(await fetchText(server, '/playlists'));
	return playlists[0].id;
}
async function podcastPath(server) {
	const html = await fetchText(server, '/library/podcasts');
	return html.match(/href="(\/library\/podcasts\/\d+)"/)[1];
}
const FEED = encodeURIComponent('http://127.0.0.1:3101/feed.xml');
const PAGES = {
	login: async () => '/login',
	start: async () => '/library',
	books: async () => '/library/books',
	'books-list': async () => '/library/books?view=list',
	'book-detail': async (s) => `/library/books/${await itemId(s, 'E2E-Fixture-Book-EndOfBook')}`,
	series: async () => '/library/series',
	albums: async () => '/library/albums',
	'albums-list': async () => '/library/albums?view=list',
	'album-detail': async (s) => `/library/albums/${await itemId(s, 'E2E-Fixture-Album-Detail')}`,
	artists: async () => '/library/artists',
	'artist-detail': async () => '/library/artists/E2E%20Fixture%20Artist',
	favorites: async () => '/library/favorites',
	bookmarks: async () => '/library/bookmarks',
	search: async () => '/library/search?q=E2E-Fixture',
	'search-empty': async () => '/library/search?q=nichtsdergleichen',
	playlists: async () => '/library/playlists',
	'playlist-detail': async (s) => `/library/playlists/${await playlistId(s)}`,
	podcasts: async () => '/library/podcasts',
	'podcast-detail': podcastPath,
	'podcast-preview': async () => `/library/podcasts/preview?feed=${FEED}`,
	player: async (s) => `/library/${await itemId(s, 'E2E-Fixture-Book-EndOfBook')}/player`,
	settings: async () => '/settings',
	'settings-libraries': async () => '/settings/libraries',
	'settings-scan': async () => '/settings/scan',
	'settings-playback': async () => '/settings/playback',
	'settings-podcasts': async () => '/settings/podcasts',
	'settings-users': async () => '/settings/users',
	'settings-security': async () => '/settings/security',
	'settings-about': async () => '/settings/about',
	'not-found': async () => '/library/books/999999'
};

async function cssFor(server, html) {
	const doc = parseDocument(html);
	const links = selectAll('link[rel="stylesheet"]', doc).map((l) =>
		l.attribs.href.replace(/^.*?(?=_app\/)/, '/')
	);
	let css = '';
	for (const href of links) css += (await inline(server, href)) + '\n';
	for (const st of selectAll('style', doc)) css += DomUtils.textContent(st) + '\n';
	return css;
}
async function inline(server, href) {
	let text = await fetchText(server, href, false);
	const base = href.slice(0, href.lastIndexOf('/') + 1);
	for (const m of [...text.matchAll(/@import\s+(?:url\()?['"]([^'"]+)['"]\)?\s*;/g)]) {
		text = text.replace(m[0], await inline(server, new URL(m[1], 'http://x' + base).pathname));
	}
	return text;
}

function minify(css) {
	const { code } = transform({
		filename: 'x.css',
		code: Buffer.from(css),
		minify: true,
		targets: { chrome: 110 << 16 },
		errorRecovery: true
	});
	return code.toString();
}

// --- tiny CSS rule parser for minified output
function parseRules(css, media = [], out = []) {
	let i = 0;
	while (i < css.length) {
		const open = css.indexOf('{', i);
		if (open < 0) break;
		const prelude = css.slice(i, open).trim();
		let depth = 1;
		let k = open + 1;
		while (depth && k < css.length) {
			if (css[k] === '{') depth++;
			else if (css[k] === '}') depth--;
			k++;
		}
		const body = css.slice(open + 1, k - 1);
		if (prelude.startsWith('@media')) parseRules(body, [...media, prelude.slice(6).trim()], out);
		else if (prelude.startsWith('@supports') || prelude.startsWith('@layer')) parseRules(body, media, out);
		else if (prelude.startsWith('@')) {
			/* keyframes, font-face */
		} else out.push({ selector: prelude, media, decls: parseDecls(body) });
		i = k;
	}
	return out;
}
function parseDecls(body) {
	const decls = [];
	let depth = 0;
	let cur = '';
	for (const ch of body) {
		if (ch === '(') depth++;
		if (ch === ')') depth--;
		if (ch === ';' && depth === 0) {
			decls.push(cur);
			cur = '';
		} else cur += ch;
	}
	if (cur.trim()) decls.push(cur);
	return decls
		.map((d) => {
			const c = d.indexOf(':');
			let value = d.slice(c + 1).trim();
			const important = /!important$/.test(value);
			value = value.replace(/\s*!important$/, '');
			return { prop: d.slice(0, c).trim(), value, important };
		})
		.filter((d) => d.prop);
}
function splitSelectors(sel) {
	const parts = [];
	let depth = 0;
	let cur = '';
	for (const ch of sel) {
		if (ch === '(' || ch === '[') depth++;
		if (ch === ')' || ch === ']') depth--;
		if (ch === ',' && depth === 0) {
			parts.push(cur);
			cur = '';
		} else cur += ch;
	}
	parts.push(cur);
	return parts.map((s) => s.trim());
}

function mediaMatches(q) {
	// handles: (max-width:700px) (width<=700px) (min-width:..) (prefers-color-scheme:x) (hover:none)
	// (prefers-reduced-motion:reduce) joined by "and", optional leading "not"/"screen"
	return q.split(/\s*,\s*/).some((one) => {
		let neg = false;
		one = one.replace(/^only\s+/, '');
		if (one.startsWith('not ')) {
			neg = true;
			one = one.slice(4);
		}
		const conds = one.split(/\s+and\s+/).map((c) => c.trim()).filter((c) => c !== 'screen' && c !== 'all');
		const ok = conds.every((c) => {
			let m;
			c = c.replace(/^\(|\)$/g, '');
			if ((m = c.match(/^max-width:\s*([\d.]+)px$/))) return WIDTH <= +m[1];
			if ((m = c.match(/^min-width:\s*([\d.]+)px$/))) return WIDTH >= +m[1];
			if ((m = c.match(/^width\s*<=\s*([\d.]+)px$/))) return WIDTH <= +m[1];
			if ((m = c.match(/^width\s*<\s*([\d.]+)px$/))) return WIDTH < +m[1];
			if ((m = c.match(/^width\s*>=\s*([\d.]+)px$/))) return WIDTH >= +m[1];
			if ((m = c.match(/^width\s*>\s*([\d.]+)px$/))) return WIDTH > +m[1];
			if ((m = c.match(/^([\d.]+)px\s*<=\s*width$/))) return WIDTH >= +m[1];
			if ((m = c.match(/^([\d.]+)px\s*<\s*width$/))) return WIDTH > +m[1];
			if ((m = c.match(/^prefers-color-scheme:\s*(\w+)$/))) return m[1] === theme;
			if ((m = c.match(/^hover:\s*(\w+)$/))) return m[1] === 'hover';
			if ((m = c.match(/^pointer:\s*(\w+)$/))) return m[1] === 'fine';
			if ((m = c.match(/^prefers-reduced-motion:\s*(\w+)$/))) return m[1] === 'no-preference';
			if (c === 'print') return false;
			console.error('?? media', c);
			return false;
		});
		return neg ? !ok : ok;
	});
}

const DYNAMIC = /:(hover|focus-visible|focus-within|focus|active|visited|disabled|checked|placeholder-shown|target|first-child|last-child|first-of-type|last-of-type|nth-child\([^)]*\)|not\([^()]*\)|empty)/g;
function prepare(rules) {
	const out = [];
	rules.forEach((rule, order) => {
		if (!rule.media.every(mediaMatches)) return;
		for (const sel of splitSelectors(rule.selector)) {
			let pseudoEl = '';
			let s = sel.replace(/::?(before|after|placeholder|selection|marker|backdrop|-webkit-[\w-]+|-moz-[\w-]+)\b(\([^)]*\))?/g, (m) => {
				pseudoEl = m.replace(/^:+/, '::');
				return '';
			});
			const states = [];
			// only strip interactive state pseudos; structural ones css-select handles itself
			s = s.replace(/:(hover|focus-visible|focus-within|focus|active|visited)\b/g, (m) => {
				states.push(m.slice(1));
				return '';
			});
			if (!s.trim() || /[>+~ ]$/.test(s.trim())) s = s + '*';
			let spec;
			try {
				const c = calculate(sel);
				const r = Array.isArray(c) ? c[0] : c;
				spec = [r.A, r.B, r.C];
			} catch (e) {
				console.error('?? specificity', sel);
				spec = [0, 0, 0];
			}
			out.push({ sel: s, orig: sel, pseudoEl, states, spec, order, decls: rule.decls });
		}
	});
	return out;
}

const INHERITED = new Set([
	'color', 'font', 'font-family', 'font-size', 'font-weight', 'font-style', 'line-height',
	'letter-spacing', 'text-align', 'white-space', 'visibility', 'cursor', 'text-transform',
	'word-break', 'overflow-wrap', 'font-variant-numeric', 'text-wrap', 'list-style', 'caret-color',
	'font-feature-settings', '-webkit-font-smoothing'
]);

function cascade(doc, rules) {
	const elements = selectAll('*', doc);
	const styles = new Map();
	for (const el of elements) styles.set(el, { base: new Map(), states: new Map() });
	for (const r of rules) {
		let matched;
		try {
			matched = selectAll(r.sel, doc);
		} catch (e) {
			continue;
		}
		for (const el of matched) {
			const st = styles.get(el);
			if (!st) continue;
			const key = (r.states.length ? r.states.join('+') : '') + (r.pseudoEl || '');
			const map = key ? (st.states.get(key) ?? st.states.set(key, new Map()).get(key)) : st.base;
			for (const d of r.decls) {
				const prev = map.get(d.prop);
				const rank = [d.important ? 1 : 0, ...r.spec, r.order];
				if (!prev || cmp(rank, prev.rank) >= 0) map.set(d.prop, { value: d.value, rank, sel: r.orig });
			}
		}
	}
	// inline styles
	for (const el of elements) {
		if (!el.attribs?.style) continue;
		for (const d of parseDecls(el.attribs.style)) {
			const prev = styles.get(el).base.get(d.prop);
			const rank = [d.important ? 1 : 0, 1, 0, 0, 0];
			if (!prev || prev.rank[0] === 0 || d.important)
				styles.get(el).base.set(d.prop, { value: d.value, rank: [0, 9, 9, 9, 0], sel: 'style=' });
		}
	}
	return { elements, styles };
}
function cmp(a, b) {
	for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return a[i] - b[i];
	return 0;
}

function effective(el, styles, prop) {
	for (let e = el; e && e.type === 'tag'; e = e.parent) {
		const v = styles.get(e)?.base.get(prop);
		if (v) return v;
		if (!INHERITED.has(prop) && !prop.startsWith('--')) return undefined;
	}
	return undefined;
}

function ownText(el) {
	return el.children
		.filter((c) => c.type === 'text')
		.map((c) => c.data)
		.join(' ')
		.replace(/\s+/g, ' ')
		.trim()
		.slice(0, 40);
}
let SHARED = new Set();
function classes(el) {
	return (el.attribs.class ?? '').split(/\s+/).filter((c) => c && !c.startsWith('svelte-'));
}
function sig(el) {
	const a = el.attribs;
	const cls = classes(el).filter((c) => SHARED.has(c)).sort().join('.');
	return [el.name, cls, a.role ?? '', a.placeholder ?? '', ownText(el).replace(/[\d.,]+/g, '#')].join('|');
}
function label(el) {
	const cls = (el.attribs.class ?? '').split(/\s+/).filter((c) => c && !c.startsWith('svelte-'));
	const t = ownText(el) || el.attribs['aria-label'] || '';
	return `${el.name}${cls.length ? '.' + cls.join('.') : ''}${t ? ` "${t.slice(0, 30)}"` : ''}`;
}
function path(el) {
	const parts = [];
	for (let e = el; e && e.type === 'tag' && e.name !== 'body'; e = e.parent) parts.unshift(label(e).split(' ')[0]);
	return parts.slice(-4).join(' > ');
}

function lcs(a, b) {
	const n = a.length;
	const m = b.length;
	const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
	for (let i = n - 1; i >= 0; i--)
		for (let j = m - 1; j >= 0; j--)
			dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
	const pairs = [];
	let i = 0;
	let j = 0;
	while (i < n && j < m) {
		if (a[i] === b[j]) {
			pairs.push([i, j]);
			i++;
			j++;
		} else if (dp[i + 1][j] >= dp[i][j + 1]) i++;
		else j++;
	}
	return pairs;
}

function visibleBody(doc) {
	const body = selectAll('body', doc)[0];
	return selectAll('*', body).filter((el) => {
		if (['script', 'style', 'template', 'svg', 'path', 'use', 'symbol', 'circle', 'line', 'polyline', 'rect', 'polygon', 'g', 'defs'].includes(el.name)) return false;
		// Layout-neutral wrappers: SvelteKit's `display: contents` root, the htmx boost root and
		// #page — leaving them out keeps the two trees aligned.
		if (el.attribs.style === 'display: contents') return false;
		if (el.attribs.id === 'page' || (el.attribs.class ?? '').split(' ').includes('boost-root')) return false;
		for (let e = el; e && e.type === 'tag'; e = e.parent) {
			if (e.attribs.hidden !== undefined) return false;
			if (['template', 'svg', 'script'].includes(e.name) && e !== el) return false;
		}
		return true;
	});
}


// --i: the visualizer's bar index; --accent: set on placeholders but read by no rule (legacy wrote
// var(--album), which does not exist).
const IGNORE_PROPS = new Set(['--i', '--accent']);
/** Numbers to two decimals: 34.27777% (Svelte) and 34.278% (askama) are the same width. */
/** Keyframes renamed by Svelte's scoping (svelte-x-bounce) or here (viz-bounce) are the same. */
const num = (v) =>
	v
		?.replace(/svelte-[a-z0-9]+-|\b(?:viz|scan)-(?=bounce|sweep)/g, '')
		.replace(/-?\d*\.?\d+/g, (n) => String(Math.round(parseFloat(n) * 100) / 100));

function compare(L, R) {
	const pairs = lcs(L.els.map(sig), R.els.map(sig));
	const unmatchedL = new Set(L.els);
	const unmatchedR = new Set(R.els);
	const out = [];
	for (const [i, j] of pairs) {
		const a = L.els[i];
		const b = R.els[j];
		unmatchedL.delete(a);
		unmatchedR.delete(b);
		const props = new Set([...L.styles.get(a).base.keys(), ...R.styles.get(b).base.keys()]);
		const diffs = [];
		for (const p of props) {
			if (IGNORE_PROPS.has(p)) continue;
			if (INHERITED.has(p) && !L.styles.get(a).base.has(p) && !R.styles.get(b).base.has(p)) continue;
			const ea = effective(a, L.styles, p);
			const eb = effective(b, R.styles, p);
			if (num(ea?.value) === num(eb?.value)) continue;
			const show = (e) => (e ? `${e.value} {${e.sel}}` : '∅');
			diffs.push(`${p}: ${show(ea)}  →  ${show(eb)}`);
		}
		if (STATES_FLAG) {
			const keys = new Set([...L.styles.get(a).states.keys(), ...R.styles.get(b).states.keys()]);
			for (const k of keys) {
				const ma = L.styles.get(a).states.get(k) ?? new Map();
				const mb = R.styles.get(b).states.get(k) ?? new Map();
				for (const p of new Set([...ma.keys(), ...mb.keys()])) {
					const va = ma.get(p)?.value;
					const vb = mb.get(p)?.value;
					if (va !== vb) diffs.push(`[${k}] ${p}: ${va ?? '∅'}  →  ${vb ?? '∅'}`);
				}
			}
		}
		if (diffs.length) out.push(`${path(a)}   ~   ${label(b).split(' ')[0]}\n     ` + diffs.join('\n     '));
	}
	const lines = [`${pairs.length} gepaart, ${unmatchedL.size} nur legacy, ${unmatchedR.size} nur rust`, ...out];
	if (flags.has('--unmatched')) {
		lines.push('-- nur legacy:', ...[...unmatchedL].map((e) => '   ' + path(e) + ' ' + label(e)));
		lines.push('-- nur rust:', ...[...unmatchedR].map((e) => '   ' + path(e) + ' ' + label(e)));
	}
	return { lines, count: out.length };
}

async function analyse(server, html) {
	const doc = parseDocument(html);
	selectAll('html', doc)[0].attribs['data-theme'] = theme;
	const rules = prepare(parseRules(minify(await cssFor(server, html))));
	const { styles } = cascade(doc, rules);
	return { styles, els: visibleBody(doc) };
}

await login('legacy');
await login('rust');
mkdirSync(OUT, { recursive: true });
const widths = (process.env.WIDTHS ?? '1440,820,390').split(',').map(Number);
const themes = (process.env.THEMES ?? 'dark,light').split(',');
for (const [name, pathOf] of Object.entries(PAGES)) {
	if (only.length && !only.includes(name)) continue;
	const html = {};
	for (const server of ['legacy', 'rust']) {
		html[server] = await fetchText(server, await pathOf(server), name !== 'login');
	}
	const summary = [];
	for (const width of widths) {
		for (const t of themes) {
			WIDTH = width;
			theme = t;
			const { lines, count } = compare(await analyse('legacy', html.legacy), await analyse('rust', html.rust));
			writeFileSync(new URL(`${name}.${width}.${t}.txt`, OUT), lines.join('\n') + '\n');
			summary.push(`${width}/${t}: ${count}`);
		}
	}
	console.log(`${name.padEnd(20)} ${summary.join('  ')}`);
}
