/**
 * Publication dates in the podcast views: the last few days read better as words
 * than as a date, and "Heute" has to mean the calendar day, not "within 24 hours".
 */
export function relativeDay(value: string | null, now: Date = new Date()): string {
	if (!value) return '';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	const midnight = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
	const days = Math.round((midnight(now) - midnight(date)) / 86_400_000);
	if (days === 0) return 'Heute';
	if (days === 1) return 'Gestern';
	if (days > 1 && days < 7) return `vor ${days} Tagen`;
	return date.toLocaleDateString('de-DE', { day: 'numeric', month: 'short', year: 'numeric' });
}

/** "48 min", "1 Std 12 min" — how long a listen takes, never a bare decimal. */
export function humanDuration(seconds: number): string {
	if (!seconds || seconds < 0) return '';
	if (seconds < 60) return '<1 min';
	const hours = Math.floor(seconds / 3600);
	const minutes = Math.round((seconds % 3600) / 60);
	if (hours === 0) return `${minutes} min`;
	return minutes === 0 ? `${hours} Std` : `${hours} Std ${minutes} min`;
}
