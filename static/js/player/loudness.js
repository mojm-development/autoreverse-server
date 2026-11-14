// "Lautstärke angleichen": the server gives every music track the gain that brings it to
// -18 LUFS (`gain_db`), the same for its album as a whole (`album_gain_db`) and its peak. The
// player turns the element's volume by that.
//
// It is done with `audio.volume`, not a Web Audio gain node: the volume needs no audio graph
// (which browsers only start after a gesture and which would sit in every stream, radio
// included), but it ends at 1.0 — the player can only turn down. That is what the low target
// is for: nearly all music is louder than -18 LUFS and gets a negative gain. A quiet recording
// that would need a boost simply plays as it is. (Safari on iOS ignores `volume` altogether;
// there nothing is levelled.)

/** A finite number, else null (the fields are `null` until the server knows a value). */
function known(value) {
	return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

/**
 * The factor a track's samples are to be multiplied with. `useAlbum` picks the album's value
 * — for an album played as such, so that its quiet and loud pieces keep their relation and
 * nothing jumps between two tracks — else the track's own; each stands in for the other where
 * only one is known. A boost is limited so that `factor × peak ≤ 1`; without a peak there is
 * no boost at all. Unknown loudness: 1.
 */
export function gainFactor(track, useAlbum) {
	const own = known(track?.gain_db);
	const album = known(track?.album_gain_db);
	const gain = useAlbum ? (album ?? own) : (own ?? album);
	if (gain === null) return 1;
	const factor = 10 ** (gain / 20);
	if (factor <= 1) return factor;
	const peak = known(track?.peak);
	const ceiling = peak !== null && peak > 0 ? Math.max(1, 1 / peak) : 1;
	return Math.min(factor, ceiling);
}

/** The same as an `audio.volume`, which cannot go above 1. */
export function volumeFor(track, useAlbum) {
	return Math.min(1, Math.max(0, gainFactor(track, useAlbum)));
}

const NORMALIZE_KEY = 'autoreverse-normalize';

/**
 * Whether the volume is levelled. Kept in this browser, like autoplay and the theme: on unless
 * switched off, and on where there is no storage to ask.
 */
export function normalizeEnabled(storage) {
	try {
		return storage.getItem(NORMALIZE_KEY) !== 'off';
	} catch {
		return true;
	}
}

export function setNormalize(storage, on) {
	try {
		storage.setItem(NORMALIZE_KEY, on ? 'on' : 'off');
	} catch {
		// No storage: the switch cannot be kept, levelling stays on.
	}
}
