//! ICY metadata: the "now playing" text SHOUTcast and Icecast senders weave into the audio.
//!
//! A sender asked with `Icy-MetaData: 1` answers with `icy-metaint: N` and then repeats: `N`
//! bytes of audio, one length byte `L`, `L × 16` bytes of text padded with NULs
//! (`StreamTitle='Artist - Title';StreamUrl='';`). A length of zero means "nothing new". The
//! blocks fall wherever the network cuts the stream into chunks, so the filter keeps its place
//! across calls. What it hands on is the audio alone: no client has to know about ICY.

/// A sender that claims a larger interval than this is not believed; its stream passes
/// untouched rather than being cut at a place nobody can check.
const MAX_INTERVAL: usize = 1 << 20;

enum Phase {
    /// This many audio bytes until the next length byte.
    Audio(usize),
    /// The next byte is the length byte.
    Length,
    /// This many bytes of the text block are still missing.
    Text(usize),
}

/// Takes the metadata blocks out of a stream that carries one every `interval` bytes.
pub struct IcyFilter {
    interval: usize,
    phase: Phase,
    text: Vec<u8>,
}

impl IcyFilter {
    /// The filter for a sender's `icy-metaint` header, or `None` when the header is missing or
    /// makes no sense — the stream is plain audio then.
    pub fn from_header(value: Option<&str>) -> Option<IcyFilter> {
        let interval: usize = value?.trim().parse().ok()?;
        (1..=MAX_INTERVAL).contains(&interval).then(|| IcyFilter {
            interval,
            phase: Phase::Audio(interval),
            text: Vec::new(),
        })
    }

    /// Appends the audio in `chunk` to `audio` and returns the text of every metadata block that
    /// ended inside it, oldest first. Empty blocks ("no change") are not reported.
    pub fn feed(&mut self, mut chunk: &[u8], audio: &mut Vec<u8>) -> Vec<Vec<u8>> {
        let mut blocks = Vec::new();
        while !chunk.is_empty() {
            match self.phase {
                Phase::Audio(left) => {
                    let take = left.min(chunk.len());
                    audio.extend_from_slice(&chunk[..take]);
                    chunk = &chunk[take..];
                    self.phase = if take == left {
                        Phase::Length
                    } else {
                        Phase::Audio(left - take)
                    };
                }
                Phase::Length => {
                    let length = usize::from(chunk[0]) * 16;
                    chunk = &chunk[1..];
                    self.text.clear();
                    self.phase = if length == 0 {
                        Phase::Audio(self.interval)
                    } else {
                        Phase::Text(length)
                    };
                }
                Phase::Text(left) => {
                    let take = left.min(chunk.len());
                    self.text.extend_from_slice(&chunk[..take]);
                    chunk = &chunk[take..];
                    if take == left {
                        blocks.push(std::mem::take(&mut self.text));
                        self.phase = Phase::Audio(self.interval);
                    } else {
                        self.phase = Phase::Text(left - take);
                    }
                }
            }
        }
        blocks
    }
}

/// UTF-8 where it is valid, else Latin-1: older senders still write "Björk" in one byte per
/// letter, and a replacement character would be all a listener sees of it.
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// The `StreamTitle` of a metadata block: `Some("")` when the sender names an empty title (a
/// jingle, the news), `None` when the block does not carry the field at all.
pub fn stream_title(block: &[u8]) -> Option<String> {
    let end = block.iter().position(|&b| b == 0).unwrap_or(block.len());
    let text = decode(&block[..end]);
    const KEY: &str = "StreamTitle='";
    let start = text.find(KEY)? + KEY.len();
    let rest = &text[start..];
    // A title may hold an apostrophe ("Don't Stop"), so the field ends at `';`, or failing that
    // at the last apostrophe of the block.
    let end = rest.find("';").or_else(|| rest.rfind('\''))?;
    Some(rest[..end].trim().to_owned())
}

/// What `/now` says about a title text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NowPlaying {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub raw: Option<String>,
}

/// "Artist - Title" taken apart where that is unambiguous: exactly one " - " with text on both
/// sides. "A - B - C" could be cut at either dash, and a show's name has none; both stay `raw`
/// only, which a client then shows as it is.
pub fn now_playing(raw: Option<&str>) -> NowPlaying {
    let Some(raw) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
        return NowPlaying::default();
    };
    let parts: Vec<&str> = raw.split(" - ").map(str::trim).collect();
    let (artist, title) = match parts.as_slice() {
        [artist, title] if !artist.is_empty() && !title.is_empty() => {
            (Some((*artist).to_owned()), Some((*title).to_owned()))
        }
        _ => (None, None),
    };
    NowPlaying {
        title,
        artist,
        raw: Some(raw.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A metadata block as a sender writes it: length byte, text, NUL padding.
    fn block(text: &str) -> Vec<u8> {
        let padded = text.len().div_ceil(16);
        let mut out = vec![u8::try_from(padded).unwrap()];
        out.extend_from_slice(text.as_bytes());
        out.resize(1 + padded * 16, 0);
        out
    }

    /// Three audio runs of `interval` bytes with a title, an empty block and a second title
    /// between them, and what a client must receive.
    fn stream(interval: usize) -> (Vec<u8>, Vec<u8>) {
        let audio: Vec<u8> = (0..interval * 3 + 5).map(|i| (i % 251) as u8).collect();
        let mut wire = Vec::new();
        wire.extend_from_slice(&audio[..interval]);
        wire.extend(block("StreamTitle='Kraftwerk - Autobahn';StreamUrl='';"));
        wire.extend_from_slice(&audio[interval..interval * 2]);
        wire.push(0);
        wire.extend_from_slice(&audio[interval * 2..interval * 3]);
        wire.extend(block("StreamTitle='Nachrichten';"));
        wire.extend_from_slice(&audio[interval * 3..]);
        (wire, audio)
    }

    fn run(wire: &[u8], interval: usize, sizes: &[usize]) -> (Vec<u8>, Vec<String>) {
        let mut filter = IcyFilter::from_header(Some(&interval.to_string())).unwrap();
        let mut audio = Vec::new();
        let mut titles = Vec::new();
        let mut rest = wire;
        let mut turn = 0;
        while !rest.is_empty() {
            let take = sizes[turn % sizes.len()].min(rest.len());
            turn += 1;
            for found in filter.feed(&rest[..take], &mut audio) {
                titles.push(stream_title(&found).unwrap());
            }
            rest = &rest[take..];
        }
        (audio, titles)
    }

    #[test]
    fn the_audio_comes_out_whole_wherever_the_chunks_are_cut() {
        let interval = 64;
        let (wire, audio) = stream(interval);
        let expected = vec!["Kraftwerk - Autobahn".to_owned(), "Nachrichten".to_owned()];
        // One piece; byte by byte; cut right before, inside and right after the length byte and
        // the text; uneven sizes that drift across every boundary.
        for sizes in [
            vec![wire.len()],
            vec![1],
            vec![interval],
            vec![interval + 1],
            vec![interval - 1],
            vec![interval + 1 + 20],
            vec![interval + 1 + 48],
            vec![7, 13, 1, 64, 3],
        ] {
            let (got, titles) = run(&wire, interval, &sizes);
            assert_eq!(got, audio, "{sizes:?}");
            assert_eq!(titles, expected, "{sizes:?}");
        }
    }

    #[test]
    fn a_block_at_the_very_end_of_a_chunk_is_reported_with_it() {
        let mut filter = IcyFilter::from_header(Some("4")).unwrap();
        let mut audio = Vec::new();
        let mut wire = b"abcd".to_vec();
        wire.extend(block("StreamTitle='X';"));
        let found = filter.feed(&wire, &mut audio);
        assert_eq!(audio, b"abcd");
        assert_eq!(found.len(), 1);
        // The next chunk starts with audio again.
        assert!(filter.feed(b"efgh", &mut audio).is_empty());
        assert_eq!(audio, b"abcdefgh");
    }

    #[test]
    fn a_missing_or_absurd_interval_means_plain_audio() {
        for header in [
            None,
            Some(""),
            Some("0"),
            Some("-5"),
            Some("abc"),
            Some("99999999999"),
        ] {
            assert!(IcyFilter::from_header(header).is_none(), "{header:?}");
        }
        assert!(IcyFilter::from_header(Some(" 16000 ")).is_some());
    }

    #[test]
    fn the_title_is_read_out_of_the_block() {
        let title = |text: &[u8]| stream_title(text);
        assert_eq!(
            title(b"StreamTitle='Daft Punk - Around the World';StreamUrl='http://x';\0\0"),
            Some("Daft Punk - Around the World".to_owned())
        );
        // An apostrophe inside the title does not end it.
        assert_eq!(
            title(b"StreamTitle='Journey - Don't Stop Believin'';"),
            Some("Journey - Don't Stop Believin'".to_owned())
        );
        assert_eq!(title(b"StreamTitle='';"), Some(String::new()));
        assert_eq!(title(b"StreamTitle='No end'"), Some("No end".to_owned()));
        assert_eq!(title(b"StreamUrl='http://x';"), None);
        // Latin-1 from an older sender.
        assert_eq!(
            title(b"StreamTitle='Bj\xf6rk - J\xf3ga';"),
            Some("Björk - Jóga".to_owned())
        );
        assert_eq!(
            title("StreamTitle='Björk - Jóga';".as_bytes()),
            Some("Björk - Jóga".to_owned())
        );
    }

    #[test]
    fn artist_and_title_are_split_only_where_there_is_one_way_to() {
        let split = now_playing(Some(" Kraftwerk - Autobahn "));
        assert_eq!(split.artist.as_deref(), Some("Kraftwerk"));
        assert_eq!(split.title.as_deref(), Some("Autobahn"));
        assert_eq!(split.raw.as_deref(), Some("Kraftwerk - Autobahn"));

        for unclear in ["Nachrichten", "A - B - C", " - Titel", "Jean-Michel Jarre"] {
            let kept = now_playing(Some(unclear));
            assert_eq!(kept.artist, None, "{unclear}");
            assert_eq!(kept.title, None, "{unclear}");
            assert_eq!(kept.raw.as_deref(), Some(unclear.trim()), "{unclear}");
        }
        assert_eq!(now_playing(Some("  ")), NowPlaying::default());
        assert_eq!(now_playing(None), NowPlaying::default());
    }
}
