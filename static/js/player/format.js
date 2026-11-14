// Time and number formats of the player pages.

const pad = (n) => String(n).padStart(2, '0');

/** `1:02:03`, or `2:03` under an hour (mini player). */
export function formatTime(seconds) {
	const s = Math.max(0, seconds || 0);
	const h = Math.floor(s / 3600);
	const m = Math.floor((s % 3600) / 60);
	const rest = Math.floor(s % 60);
	return h > 0 ? `${h}:${pad(m)}:${pad(rest)}` : `${m}:${pad(rest)}`;
}

/** Always with hours: `0:02:03` (fullscreen player, chapter lists). */
export function formatHMS(seconds) {
	const s = Math.max(0, seconds || 0);
	const h = Math.floor(s / 3600);
	const m = Math.floor((s % 3600) / 60);
	const rest = Math.floor(s % 60);
	return `${h}:${pad(m)}:${pad(rest)}`;
}

/** `1,25×` — two decimals, German comma. */
export function formatSpeed(speed) {
	return `${speed.toLocaleString('de-DE', { minimumFractionDigits: 2 })}×`;
}

/** "12.5 Std" from an hour on, "48 min" below (book detail). */
export function formatHours(seconds) {
	const hours = seconds / 3600;
	return hours >= 1 ? `${hours.toFixed(1)} Std` : `${Math.round(seconds / 60)} min`;
}
