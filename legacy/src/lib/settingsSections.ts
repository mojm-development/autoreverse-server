export const SETTINGS_SECTIONS = [
	{
		href: '/settings/libraries',
		label: 'Bibliotheken',
		description: 'Pfade zu den Hörbuch- und Musikordnern festlegen.',
		admin: true
	},
	{
		href: '/settings/scan',
		label: 'Scan',
		description: 'Bibliothek neu einlesen und den Fortschritt verfolgen.',
		admin: true
	},
	{
		href: '/settings/users',
		label: 'Nutzer',
		description: 'Konten anlegen und Verwalterrechte vergeben.',
		admin: true
	},
	{
		href: '/settings/playback',
		label: 'Wiedergabe',
		description: 'Geschwindigkeit und Sprungweiten für die Wiedergabe anpassen.',
		admin: false
	},
	{
		href: '/settings/podcasts',
		label: 'Podcast-Abos',
		description: 'Feeds abonnieren, aktualisieren und wieder entfernen.',
		admin: true
	},
	{
		href: '/settings/security',
		label: 'Sicherheit',
		description: 'Eigenes Passwort ändern.',
		admin: false
	},
	{
		href: '/settings/about',
		label: 'Über Autoreverse',
		description: 'Version und Projektinformationen.',
		admin: false
	}
] as const;
