// Sharing helpers of static/js/share.js. Run: node --test 'tests/js/*.test.mjs'
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { absoluteLink, importName, importSummary } from '../../static/js/share.js';

test('a copied link is the page under the origin the visitor uses', () => {
	assert.equal(
		absoluteLink('/library/playlists/7', 'https://audio.example.com'),
		'https://audio.example.com/library/playlists/7'
	);
	assert.equal(
		absoluteLink('/library/albums/3?track=12', 'http://192.168.1.5:3000'),
		'http://192.168.1.5:3000/library/albums/3?track=12'
	);
	assert.equal(
		absoluteLink('/library/albums?artist=AC%2FDC', 'http://localhost:3000'),
		'http://localhost:3000/library/albums?artist=AC%2FDC'
	);
});

test('an imported playlist is named by its #PLAYLIST line, else by its file', () => {
	assert.equal(importName('zug.m3u8', '#EXTM3U\r\n#PLAYLIST: Zugfahrt \r\na.mp3'), 'Zugfahrt');
	assert.equal(importName('Zug fahrt.M3U', '#EXTM3U\na.mp3'), 'Zug fahrt');
	assert.equal(importName('liste.txt', '#PLAYLIST:\na.mp3'), 'liste');
	assert.equal(importName('.m3u', ''), 'Importierte Playlist');
	assert.equal(importName('x'.repeat(150) + '.m3u', '').length, 100);
});

test('the summary counts what was found', () => {
	assert.equal(importSummary('Zug', 12, 0), '„Zug“ angelegt: alle 12 Titel gefunden.');
	assert.equal(importSummary('Zug', 12, 3), '„Zug“ angelegt: 12 von 15 Titeln gefunden.');
});
