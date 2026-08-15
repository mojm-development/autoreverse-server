export interface PlayerTrack {
	id: number;
	position: number;
	title: string | null;
	duration: number;
}
export interface PlayerState {
	itemId: number;
	sessionId: string;
	tracks: PlayerTrack[];
	chapters: { title: string; start: number; end: number }[];
	trackIndex: number;
	position: number; // seconds, absolute across all tracks of the item
	playing: boolean;
	speed: number;
}

export function createPlayerStore() {
	let current = $state<PlayerState | null>(null);
	let heartbeat: ReturnType<typeof setInterval> | null = null;

	function stopHeartbeat() {
		if (heartbeat) clearInterval(heartbeat);
		heartbeat = null;
	}

	function startHeartbeat() {
		stopHeartbeat();
		heartbeat = setInterval(() => {
			if (current && current.playing) void flushProgress(false);
		}, 15_000);
	}

	async function flushProgress(finished: boolean) {
		if (!current) return;
		await fetch(`/progress/${current.itemId}`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ position: current.position, finished })
		});
	}

	async function play(itemId: number) {
		const res = await fetch(`/play/${itemId}`, { method: 'POST' });
		const body = await res.json();
		current = {
			itemId,
			sessionId: body.session_id,
			tracks: body.tracks,
			chapters: body.chapters,
			trackIndex: 0,
			position: body.start_position,
			playing: true,
			speed: 1
		};
		startHeartbeat();
	}

	function pause() {
		if (current) current.playing = false;
	}
	function resume() {
		if (current) current.playing = true;
	}
	function seek(seconds: number) {
		if (current) current.position = seconds;
	}
	function skipBack(seconds: number) {
		if (current) current.position = Math.max(0, current.position - seconds);
	}
	function skipForward(seconds: number) {
		if (current) current.position = current.position + seconds;
	}

	async function close() {
		if (!current) return;
		await flushProgress(false);
		await fetch(`/sessions/${current.sessionId}/close`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ position: current.position, duration: current.position })
		});
		stopHeartbeat();
		current = null;
	}

	return {
		get current() {
			return current;
		},
		play,
		pause,
		resume,
		seek,
		skipBack,
		skipForward,
		close
	};
}

export type PlayerStore = ReturnType<typeof createPlayerStore>;
export const PLAYER_CONTEXT_KEY = Symbol('player');
