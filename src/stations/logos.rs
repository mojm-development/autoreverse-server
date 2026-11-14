//! Station logos on the server's own disk, `<data>/radio/logos/<station id>.<ext>`.
//!
//! Fetched once when a station is added — from the directory, or from the station's homepage
//! where the directory has none (`site_icon`) — and served from here ever after, so no client
//! loads an image from a third party just because it shows the station list.
//!
//! What counts as a logo is decided here as well ([`normalize`]), for the stored ones and for
//! those of the search hits (`search_logos`) alike: JPEG, PNG, WebP and GIF as they come, a
//! Windows icon (`.ico`, what a directory full of `favicon.ico` addresses often names) as the
//! PNG of its largest picture. SVG is refused: it can carry scripts and references to other
//! servers, so it could only be passed on rendered or cleaned, and the server has neither a
//! renderer nor would the native clients or the thumbnails get anything out of cleaned markup.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use axum::body::Bytes;
use image::{DynamicImage, ImageFormat};

use super::search_logos::SearchLogos;
use super::store::{self, Station};
use crate::state::AppState;

/// Logos are small pictures; one that claims more is a mistake or an attempt to make the server
/// allocate.
const MAX_EDGE: u32 = 4096;
const MAX_DECODE_BYTES: u64 = 96 * 1024 * 1024;

/// A logo as it is stored and served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub extension: &'static str,
    pub content_type: &'static str,
    pub data: Bytes,
}

fn decode(bytes: &[u8], format: ImageFormat) -> Option<DynamicImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    reader.decode().ok()
}

fn png(image: &DynamicImage) -> Option<Picture> {
    let mut data = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut data), ImageFormat::Png)
        .ok()?;
    Some(Picture {
        extension: "png",
        content_type: "image/png",
        data: data.into(),
    })
}

/// An icon file holding only the largest picture of `bytes`. The `image` crate would pick by
/// colour depth first, and the depth field is often zero or wrong for the PNG entries that hold
/// the large sizes — which is how a 16-pixel picture would win over the 256-pixel one.
fn largest_ico_entry(bytes: &[u8]) -> Option<Vec<u8>> {
    const HEADER: usize = 6;
    const ENTRY: usize = 16;
    let u16_at = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let count = usize::from(u16_at(4)?);
    // A zero stands for 256.
    let edge = |raw: u8| if raw == 0 { 256 } else { u32::from(raw) };
    let best = (0..count)
        .filter_map(|index| {
            let at = HEADER + index * ENTRY;
            let entry = bytes.get(at..at + ENTRY)?;
            let area = edge(entry[0]) * edge(entry[1]);
            let start = usize::try_from(u32_at(at + 12)?).ok()?;
            let length = usize::try_from(u32_at(at + 8)?).ok()?;
            let data = bytes.get(start..start.checked_add(length)?)?;
            Some(((area, u16_at(at + 6)?), entry, data))
        })
        .max_by_key(|(score, ..)| *score)?;
    let (_, entry, data) = best;
    let mut single = Vec::with_capacity(HEADER + ENTRY + data.len());
    single.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    single.extend_from_slice(&entry[..12]);
    single.extend_from_slice(&((HEADER + ENTRY) as u32).to_le_bytes());
    single.extend_from_slice(data);
    Some(single)
}

/// The logo in `bytes`, or `None` when they are no picture the server can show: told by the
/// content, never by a name or a header, and decoded once in full so that a file which only
/// begins like a picture is not handed to a client as one. CPU work — call it off the runtime.
pub fn normalize(bytes: Vec<u8>) -> Option<Picture> {
    let format = image::guess_format(&bytes).ok()?;
    let (extension, content_type) = match format {
        ImageFormat::Jpeg => ("jpg", "image/jpeg"),
        ImageFormat::Png => ("png", "image/png"),
        ImageFormat::WebP => ("webp", "image/webp"),
        ImageFormat::Gif => ("gif", "image/gif"),
        // No client is asked to read icon files: the largest picture inside goes out as PNG.
        ImageFormat::Ico => return png(&decode(&largest_ico_entry(&bytes)?, format)?),
        _ => return None,
    };
    decode(&bytes, format)?;
    Some(Picture {
        extension,
        content_type,
        data: bytes.into(),
    })
}

/// The shorter edge of `picture` in pixels, read from its header; 0 when that says nothing.
pub fn shorter_edge(picture: &Picture) -> u32 {
    image::ImageReader::new(Cursor::new(&picture.data))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .map_or(0, |(width, height)| width.min(height))
}

/// `picture` no larger than `size`×`size`, or `None` when it is that small already. PNG where
/// the logo is transparent — on a JPEG its background would turn black — else JPEG.
pub fn scaled(picture: &Picture, size: u32) -> Option<Picture> {
    let format = image::guess_format(&picture.data).ok()?;
    let image = decode(&picture.data, format)?;
    if image.width() <= size && image.height() <= size {
        return None;
    }
    let thumb = image.thumbnail(size, size);
    if thumb.color().has_alpha() {
        return png(&thumb);
    }
    let mut data = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 85)
        .encode_image(&thumb.into_rgb8())
        .ok()?;
    Some(Picture {
        extension: "jpg",
        content_type: "image/jpeg",
        data: data.into(),
    })
}

pub fn logos_dir(radio_dir: &Path) -> PathBuf {
    radio_dir.join("logos")
}

/// Where the station's logo is, if it has one. Only the file name is taken from the row, so a
/// stored value can never point outside the logo directory.
pub fn logo_path(radio_dir: &Path, station: &Station) -> Option<PathBuf> {
    let name = Path::new(station.logo_file.as_deref()?).file_name()?;
    Some(logos_dir(radio_dir).join(name))
}

/// Writes a logo and returns its file name for the row.
pub async fn store(
    radio_dir: &Path,
    station_id: i32,
    extension: &str,
    bytes: &[u8],
) -> std::io::Result<String> {
    let dir = logos_dir(radio_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let name = format!("{station_id}.{extension}");
    // Written beside and moved into place, so a request never reads half a file.
    let partial = dir.join(format!("{name}.part-{}", std::process::id()));
    tokio::fs::write(&partial, bytes).await?;
    tokio::fs::rename(&partial, dir.join(&name)).await?;
    Ok(name)
}

/// Deletes the logo of a station that was removed. A file that is gone already is fine.
pub async fn remove(radio_dir: &Path, station: &Station) {
    if let Some(path) = logo_path(radio_dir, station) {
        let _ = tokio::fs::remove_file(path).await;
    }
}

/// Stores `picture` as the logo of station `station_id` and returns the row as it is now;
/// `None` when the station was removed in the meantime (the file goes with it).
pub async fn attach(
    state: &AppState,
    station_id: i32,
    picture: &Picture,
) -> Result<Option<Station>, sqlx::Error> {
    let radio_dir = &state.config.radio_dir;
    let file = match store(radio_dir, station_id, picture.extension, &picture.data).await {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("[radio] Logo für Sender {station_id}: {e}");
            return Ok(None);
        }
    };
    let station = store::set_logo(&state.db, station_id, &file).await?;
    if station.is_none() {
        let _ = tokio::fs::remove_file(logos_dir(radio_dir).join(&file)).await;
    }
    Ok(station)
}

/// Where the logo of a stored station that has none may still come from.
enum Source {
    /// The directory's logo, else the icon of the homepage the directory names.
    Directory(String),
    /// The icon of the station's homepage.
    Homepage(String),
}

/// Catches up on the logos of `stations` that have none: those from the directory — added
/// before icon files were understood, before homepages were looked at, or while the logo's
/// server was down — and those added by address with a homepage. Runs in the background, so the
/// list that triggered it is not kept waiting; the logos are there on a later call. Each station
/// is tried once in a while only (`SearchLogos::claim`). A server without the directory does
/// not ask it; the homepage a station carries is still looked at.
pub fn backfill(state: &AppState, stations: &[Station]) {
    let due: Vec<(i32, Source)> = stations
        .iter()
        .filter(|station| station.logo_file.is_none())
        .filter_map(|station| {
            let source = match (&station.directory_id, &station.homepage) {
                (Some(id), _) if state.settings.get().radio_directory_on() => {
                    Source::Directory(id.clone())
                }
                (_, Some(homepage)) => Source::Homepage(homepage.clone()),
                _ => return None,
            };
            Some((station.id, source))
        })
        .filter(|(id, _)| state.radio_logos.claim(*id))
        .collect();
    if due.is_empty() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        for (station_id, source) in due {
            let logo = match source {
                Source::Directory(id) => SearchLogos::get(&state, &id).await,
                Source::Homepage(homepage) => SearchLogos::of_homepage(&state, &homepage).await,
            };
            let Some(logo) = logo else { continue };
            if let Err(e) = attach(&state, station_id, &logo.original).await {
                tracing::warn!("[radio] Logo für Sender {station_id}: {e}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn encoded(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut data = Vec::new();
        image.write_to(&mut Cursor::new(&mut data), format).unwrap();
        data
    }

    fn plain(edge: u32, colour: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(edge, edge, Rgba(colour))
    }

    /// An icon file with one entry per picture, each stored as PNG or as a bitmap.
    fn ico(pictures: &[(&RgbaImage, bool)]) -> Vec<u8> {
        use image::codecs::ico::{IcoEncoder, IcoFrame};
        let frames: Vec<IcoFrame> = pictures
            .iter()
            .map(|(picture, as_png)| {
                if *as_png {
                    IcoFrame::as_png(
                        picture.as_raw(),
                        picture.width(),
                        picture.height(),
                        image::ExtendedColorType::Rgba8,
                    )
                    .unwrap()
                } else {
                    let single = encoded(picture, ImageFormat::Ico);
                    // The bitmap of a one-entry icon the encoder wrote: everything after
                    // header and entry.
                    IcoFrame::with_encoded(
                        single[22..].to_vec(),
                        picture.width(),
                        picture.height(),
                        image::ExtendedColorType::Rgba8,
                    )
                    .unwrap()
                }
            })
            .collect();
        let mut data = Vec::new();
        IcoEncoder::new(&mut data).encode_images(&frames).unwrap();
        data
    }

    #[test]
    fn the_usual_formats_are_kept_as_they_come() {
        let image = plain(8, [200, 30, 30, 255]);
        for (format, extension, content_type) in [
            (ImageFormat::Png, "png", "image/png"),
            (ImageFormat::Jpeg, "jpg", "image/jpeg"),
            (ImageFormat::Gif, "gif", "image/gif"),
            (ImageFormat::WebP, "webp", "image/webp"),
        ] {
            let bytes = if format == ImageFormat::Jpeg {
                let rgb = DynamicImage::ImageRgba8(image.clone()).into_rgb8();
                let mut data = Vec::new();
                rgb.write_to(&mut Cursor::new(&mut data), format).unwrap();
                data
            } else {
                encoded(&image, format)
            };
            let picture = normalize(bytes.clone()).unwrap();
            assert_eq!(picture.extension, extension);
            assert_eq!(picture.content_type, content_type);
            assert_eq!(picture.data, bytes);
        }
    }

    #[test]
    fn an_icon_file_becomes_the_png_of_its_largest_picture() {
        let small = plain(16, [0, 0, 255, 255]);
        let large = plain(64, [0, 255, 0, 128]);
        let middle = plain(32, [255, 0, 0, 255]);
        // The large one in the middle, and as PNG as well as a bitmap.
        for large_as_png in [true, false] {
            let bytes = ico(&[(&small, false), (&large, large_as_png), (&middle, true)]);
            let picture = normalize(bytes).unwrap();
            assert_eq!(
                (picture.extension, picture.content_type),
                ("png", "image/png")
            );
            assert_eq!(
                image::guess_format(&picture.data).unwrap(),
                ImageFormat::Png
            );
            let decoded = image::load_from_memory(&picture.data).unwrap().into_rgba8();
            assert_eq!((decoded.width(), decoded.height()), (64, 64));
            // Transparency survives.
            assert_eq!(decoded.get_pixel(5, 5), &Rgba([0, 255, 0, 128]));
        }
    }

    #[test]
    fn what_is_no_picture_is_no_logo() {
        let png = encoded(&plain(64, [1, 2, 3, 255]), ImageFormat::Png);
        let mut icon = ico(&[(&plain(16, [1, 2, 3, 255]), true)]);
        let entries_end = 22;
        icon.truncate(entries_end + 10);
        let refused: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"/>".to_vec(),
            b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
            b"<!doctype html><title>404</title>".to_vec(),
            // Only the signature of a PNG, and a PNG cut off in the middle.
            png[..8].to_vec(),
            png[..png.len() / 2].to_vec(),
            // An icon file without entries, one cut off, one pointing outside itself.
            vec![0, 0, 1, 0, 0, 0],
            icon,
            vec![
                0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0, 0xff, 0xff, 0xff, 0xff, 22, 0, 0, 0,
            ],
            // A bitmap is a picture, but none anybody uses as a logo on the web.
            encoded(&plain(4, [1, 2, 3, 255]), ImageFormat::Bmp),
        ];
        for bytes in refused {
            assert_eq!(normalize(bytes.clone()), None, "{bytes:?}");
        }
    }

    #[test]
    fn a_picture_larger_than_any_logo_is_refused() {
        let wide = RgbaImage::new(MAX_EDGE + 1, 1);
        assert_eq!(normalize(encoded(&wide, ImageFormat::Png)), None);
    }

    #[test]
    fn scaling_keeps_transparency_and_leaves_small_pictures_alone() {
        let opaque = DynamicImage::ImageRgba8(plain(400, [9, 9, 9, 255])).into_rgb8();
        let mut data = Vec::new();
        opaque
            .write_to(&mut Cursor::new(&mut data), ImageFormat::Png)
            .unwrap();
        let opaque = normalize(data).unwrap();
        let thumb = scaled(&opaque, 128).unwrap();
        assert_eq!(thumb.content_type, "image/jpeg");
        let decoded = image::load_from_memory(&thumb.data).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (128, 128));
        assert_eq!(scaled(&opaque, 512), None);
        assert_eq!((shorter_edge(&opaque), shorter_edge(&thumb)), (400, 128));

        let clear = normalize(encoded(&plain(400, [9, 9, 9, 0]), ImageFormat::Png)).unwrap();
        let thumb = scaled(&clear, 128).unwrap();
        assert_eq!(thumb.content_type, "image/png");
        let decoded = image::load_from_memory(&thumb.data).unwrap().into_rgba8();
        assert_eq!(decoded.width(), 128);
        assert_eq!(decoded.get_pixel(0, 0)[3], 0);
    }
}
