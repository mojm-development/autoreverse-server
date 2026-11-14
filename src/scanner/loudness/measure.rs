//! Measuring a file's loudness: `ffmpeg` decodes it to PCM on a pipe, the `ebur128` crate does
//! the EBU R128 arithmetic in this process.
//!
//! Why ffmpeg decodes: it is in the image anyway (m4b chapters, podcast chapters) and reads
//! every format the scanner accepts, Opus and HE-AAC included — which no pure-Rust decoder does
//! today. Why the arithmetic is not ffmpeg's `ebur128` filter as well: an album's loudness is
//! the gated loudness over all of its tracks together, and that needs the tracks' measuring
//! states side by side ([`album_loudness`]), not one number per file.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use ebur128::{Channel, EbuR128, Mode};

/// One decoded file. The state is kept for the album's value.
pub struct Measured {
    state: EbuR128,
    /// Integrated loudness in LUFS; `None` for a file that is silent throughout (nothing gets
    /// past the gate, the loudness is minus infinity).
    pub lufs: Option<f64>,
    /// Highest sample, linear. Lossy files do go above 1.0.
    pub peak: f64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MeasureError {
    /// `ffmpeg` cannot be started at all: nothing can be measured, and no file is to blame.
    NoDecoder(String),
    /// This file gave no audio.
    Undecodable(String),
    Cancelled,
}

/// Kills the decoder when the measurement ends early (cancelled, a broken header), so it does
/// not sit on a full pipe forever.
struct Decoder(Child);

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Fills `buffer` as far as the stream goes; less than its length means the end.
fn read_full(reader: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Channels and sample rate from the head of a WAV stream, leaving the reader at the first
/// sample. ffmpeg puts a `LIST` chunk between `fmt ` and `data`; whatever is there is skipped.
fn wav_format(reader: &mut impl Read) -> Option<(u32, u32)> {
    let mut riff = [0u8; 12];
    if read_full(reader, &mut riff).ok()? < 12 || &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
        return None;
    }
    let mut format = None;
    loop {
        let mut head = [0u8; 8];
        if read_full(reader, &mut head).ok()? < 8 {
            return None;
        }
        let size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if &head[..4] == b"data" {
            return format;
        }
        // Chunks are padded to an even length. No header chunk is anywhere near this large.
        let padded = size.checked_add(size % 2).filter(|s| *s <= 1 << 20)?;
        let mut body = vec![0u8; padded];
        if read_full(reader, &mut body).ok()? < padded {
            return None;
        }
        if &head[..4] == b"fmt " && body.len() >= 8 {
            let channels = u32::from(u16::from_le_bytes([body[2], body[3]]));
            let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            format = Some((channels, rate));
        }
    }
}

/// Feeds a stream of 32-bit float WAV into a fresh R128 state.
fn measure_wav(
    reader: &mut impl Read,
    cancelled: &dyn Fn() -> bool,
) -> Result<Measured, MeasureError> {
    let undecodable = |why: &str| MeasureError::Undecodable(why.to_owned());
    let (channels, rate) = wav_format(reader).ok_or_else(|| undecodable("kein Ton"))?;
    // The histogram keeps the memory of a state fixed however long the file is — an album's
    // worth of states is held at once.
    let mut state = EbuR128::new(
        channels,
        rate,
        Mode::I | Mode::SAMPLE_PEAK | Mode::HISTOGRAM,
    )
    .map_err(|e| undecodable(&e.to_string()))?;
    if channels == 1 {
        // A mono file comes out of both speakers; measured as a single channel it would read
        // 3 dB quieter than it sounds.
        let _ = state.set_channel(0, Channel::DualMono);
    }
    let frame = channels as usize * 4;
    // About a third of a second of CD audio per read: cancelling is noticed quickly.
    let mut bytes = vec![0u8; frame * 16384];
    let mut samples = vec![0f32; channels as usize * 16384];
    let mut frames: u64 = 0;
    loop {
        if cancelled() {
            return Err(MeasureError::Cancelled);
        }
        let filled = read_full(reader, &mut bytes).map_err(|e| undecodable(&e.to_string()))?;
        let whole = filled - filled % frame;
        if whole == 0 {
            break;
        }
        let count = whole / 4;
        for (sample, raw) in samples.iter_mut().zip(bytes[..whole].as_chunks::<4>().0) {
            let value = f32::from_le_bytes(*raw);
            // A decoder's NaN would poison the whole measurement.
            *sample = if value.is_finite() { value } else { 0.0 };
        }
        state
            .add_frames_f32(&samples[..count])
            .map_err(|e| undecodable(&e.to_string()))?;
        frames += (whole / frame) as u64;
        if filled < bytes.len() {
            break;
        }
    }
    if frames == 0 {
        return Err(undecodable("kein Ton"));
    }
    let lufs = state.loudness_global().ok().filter(|l| l.is_finite());
    let peak = (0..channels)
        .filter_map(|channel| state.sample_peak(channel).ok())
        .fold(0.0, f64::max);
    Ok(Measured { state, lufs, peak })
}

/// Decodes the first audio stream of `path` completely and measures it. `cancelled` is asked
/// several times a second of work; the decoder is killed when it says yes.
pub fn measure_file(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<Measured, MeasureError> {
    let child = Command::new("ffmpeg")
        .args(["-v", "quiet", "-nostdin", "-i"])
        .arg(path)
        // The first audio stream only (an embedded cover is a video stream), as it is: no
        // resampling and no downmix, both would cost time and move the peak.
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn"])
        .args(["-c:a", "pcm_f32le", "-f", "wav", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| MeasureError::NoDecoder(e.to_string()))?;
    let mut decoder = Decoder(child);
    let mut stdout = decoder
        .0
        .stdout
        .take()
        .ok_or_else(|| MeasureError::NoDecoder("keine Ausgabe von ffmpeg".into()))?;
    // A file that breaks off halfway is measured as far as it plays: that is what is heard.
    measure_wav(&mut stdout, cancelled)
}

/// The loudness of several files as one programme: R128's gating over all of their blocks
/// together, which is what "album gain" means — not the mean of the tracks' values, which
/// would let a short quiet interlude pull a loud album up.
pub fn album_loudness(tracks: &[&Measured]) -> Option<f64> {
    EbuR128::loudness_global_multiple(tracks.iter().map(|m| &m.state))
        .ok()
        .filter(|l| l.is_finite())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 32-bit float WAV of a sine at `amplitude` (linear), as ffmpeg would pipe it.
    pub(crate) fn sine_wav(seconds: f64, hz: f64, amplitude: f64, channels: u16) -> Vec<u8> {
        let rate = 48_000u32;
        let frames = (seconds * f64::from(rate)) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
        out.extend_from_slice(&(channels * 4).to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        // An odd-sized chunk in between, as ffmpeg's LIST chunk can be.
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&[1, 2, 3, 0]);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        for n in 0..frames {
            let t = f64::from(n) / f64::from(rate);
            let value = (amplitude * (std::f64::consts::TAU * hz * t).sin()) as f32;
            for _ in 0..channels {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out
    }

    fn measure(bytes: &[u8]) -> Result<Measured, MeasureError> {
        measure_wav(&mut &bytes[..], &|| false)
    }

    /// Linear amplitude for a level in dBFS.
    pub(crate) fn dbfs(level: f64) -> f64 {
        10f64.powf(level / 20.0)
    }

    #[test]
    fn a_stereo_sine_at_minus_23_dbfs_reads_minus_23_lufs() {
        // EBU Tech 3341, case 1: a 1 kHz stereo sine at -23 dBFS is -23.0 LUFS.
        let found = measure(&sine_wav(20.0, 1000.0, dbfs(-23.0), 2)).unwrap();
        assert!((found.lufs.unwrap() + 23.0).abs() < 0.1, "{:?}", found.lufs);
        assert!((found.peak - dbfs(-23.0)).abs() < 0.001, "{}", found.peak);
    }

    #[test]
    fn mono_is_measured_as_it_sounds_from_two_speakers() {
        let mono = measure(&sine_wav(10.0, 1000.0, dbfs(-20.0), 1)).unwrap();
        let stereo = measure(&sine_wav(10.0, 1000.0, dbfs(-20.0), 2)).unwrap();
        assert!((mono.lufs.unwrap() - stereo.lufs.unwrap()).abs() < 0.05);
        assert!((stereo.lufs.unwrap() + 20.0).abs() < 0.1);
    }

    #[test]
    fn silence_has_no_loudness_but_is_not_a_failure() {
        let found = measure(&sine_wav(5.0, 1000.0, 0.0, 2)).unwrap();
        assert_eq!(found.lufs, None);
        assert_eq!(found.peak, 0.0);
    }

    #[test]
    fn an_album_is_gated_as_one_programme() {
        // Ten seconds at -10 dBFS and ten at -30: the relative gate sits 10 LU below the mean
        // of both, and the quiet half falls under it. So the album reads as loud as its loud
        // track, far from the -20 a mean of the two values would give.
        let loud = measure(&sine_wav(10.0, 1000.0, dbfs(-10.0), 2)).unwrap();
        let quiet = measure(&sine_wav(10.0, 1000.0, dbfs(-30.0), 2)).unwrap();
        let album = album_loudness(&[&loud, &quiet]).unwrap();
        assert!((album + 10.0).abs() < 0.3, "{album}");
        // Two tracks close together do average, in energy.
        let near = measure(&sine_wav(10.0, 1000.0, dbfs(-14.0), 2)).unwrap();
        let both = album_loudness(&[&loud, &near]).unwrap();
        assert!(both < -10.0 && both > -14.0, "{both}");
        assert_eq!(album_loudness(&[]), None);
    }

    #[test]
    fn something_that_is_not_audio_is_undecodable() {
        assert!(matches!(
            measure(b"das ist keine Musik"),
            Err(MeasureError::Undecodable(_))
        ));
        assert!(matches!(measure(b""), Err(MeasureError::Undecodable(_))));
        // A header without a single sample.
        let empty = sine_wav(0.0, 1000.0, 0.5, 2);
        assert!(matches!(measure(&empty), Err(MeasureError::Undecodable(_))));
    }

    #[test]
    fn cancelling_stops_the_measurement() {
        let bytes = sine_wav(5.0, 1000.0, 0.5, 2);
        let result = measure_wav(&mut &bytes[..], &|| true);
        assert!(matches!(result, Err(MeasureError::Cancelled)));
    }
}
