// "Streaming-Qualität" in the web player: how much of a track is asked of the server
// (`GET /tracks/{id}/stream?quality=…&format=…`, README "Streaming quality"). The library is
// mostly FLAC; over a slow line a smaller copy is what lets a title begin at once.

const QUALITY_KEY = 'autoreverse-quality';
const MODES = ['auto', 'original', 'saver'];

/** Mbit/s. Below the first the line is slow, below the second very slow. */
export const SLOW = 1.5;
export const VERY_SLOW = 0.3;

/**
 * What the listener chose: `auto`, `original` or `saver`. Kept in this browser, like autoplay
 * and the theme; automatic unless something else was stored, and where there is no storage.
 */
export function qualityMode(storage) {
	try {
		const stored = storage.getItem(QUALITY_KEY);
		return MODES.includes(stored) ? stored : 'auto';
	} catch {
		return 'auto';
	}
}

export function setQualityMode(storage, mode) {
	try {
		storage.setItem(QUALITY_KEY, MODES.includes(mode) ? mode : 'auto');
	} catch {
		// No storage: the choice cannot be kept, it stays automatic.
	}
}

/**
 * The `quality` to ask for; `null` is the file as it is. Automatic goes by what the browser
 * says about the line (`navigator.connection` — not every browser has it): its data-saving
 * switch, or how fast it reckons the line to be. Where it says nothing, the original.
 *
 * @param {string} mode
 * @param {{ saveData?: boolean, downlink?: number } | null | undefined} connection
 */
export function pickQuality(mode, connection) {
	if (mode === 'original') return null;
	if (mode === 'saver') return 'low';
	if (!connection) return null;
	if (connection.saveData) return 'low';
	const downlink = connection.downlink;
	if (typeof downlink !== 'number' || !Number.isFinite(downlink) || downlink <= 0) return null;
	if (downlink < VERY_SLOW) return 'low';
	if (downlink < SLOW) return 'medium';
	return null;
}

/**
 * Opus where the browser plays it, AAC elsewhere (Safari before 18.4 reads no Ogg).
 * `canPlay` is the audio element's `canPlayType`.
 */
export function pickFormat(canPlay) {
	return canPlay('audio/ogg; codecs="opus"') ? 'opus' : 'aac';
}

/** The address of a track's audio in `quality` (`null`: as it is). */
export function trackUrl(trackId, quality, format) {
	const base = `/tracks/${trackId}/stream`;
	return quality ? `${base}?quality=${quality}&format=${format}` : base;
}
