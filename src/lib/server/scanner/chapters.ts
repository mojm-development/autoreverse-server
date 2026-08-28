import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

export interface Chapter {
	title: string;
	start: number;
	end: number;
}

export async function readChapters(path: string): Promise<Chapter[]> {
	let stdout: string;
	try {
		const result = await execFileAsync(
			'ffprobe',
			['-v', 'quiet', '-print_format', 'json', '-show_chapters', path],
			{ timeout: 30_000 }
		);
		stdout = result.stdout;
	} catch {
		return [];
	}
	try {
		const parsed = JSON.parse(stdout) as { chapters?: unknown[] };
		const entries = parsed.chapters ?? [];
		return entries.map((entry: unknown, index: number) => {
			const entryObj = entry as Record<string, unknown>;
			const start = Number(entryObj.start_time);
			const end = Number(entryObj.end_time);
			if (!Number.isFinite(start) || !Number.isFinite(end)) throw new Error('malformed chapter');
			const tags = entryObj.tags as Record<string, unknown> | undefined;
			return { title: (tags?.title as string | undefined) || `Kapitel ${index + 1}`, start, end };
		});
	} catch {
		return [];
	}
}
