//! Adding a station: from the directory by its id, or by an address somebody typed in. Shared
//! by the JSON API and the web UI.

use super::directory::{self, DirectoryStation};
use super::logos;
use super::probe::{self, Probed, codec_of};
use super::search_logos::SearchLogos;
use super::store::{self, Added, NewStation, Station};
use crate::state::AppState;

pub const DIRECTORY_OFF: &str = "Die Sendersuche ist auf diesem Server abgeschaltet";
pub const UNKNOWN_DIRECTORY_STATION: &str = "Diesen Sender kennt das Verzeichnis nicht (mehr)";
pub const NAME_INVALID: &str = "Der Name muss 1–100 Zeichen lang sein.";
pub const HOMEPAGE_INVALID: &str = "Die Webseite muss mit http:// oder https:// beginnen.";
pub const LIST_FULL: &str = "Mehr als 200 Sender sind nicht möglich.";

#[derive(Debug, thiserror::Error)]
pub enum AddError {
    /// The address or a field is not acceptable; a sentence for the user (422).
    #[error("{0}")]
    Invalid(String),
    /// The directory search is switched off (404).
    #[error("{DIRECTORY_OFF}")]
    DirectoryOff,
    /// The directory has no station of this id (404).
    #[error("{UNKNOWN_DIRECTORY_STATION}")]
    UnknownStation,
    /// The directory could not be asked (502).
    #[error("{0}")]
    Directory(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// A station name: trimmed, 1–100 characters.
pub fn valid_name(raw: &str) -> Option<&str> {
    let name = raw.trim();
    (1..=100).contains(&name.chars().count()).then_some(name)
}

/// Stores `new` and looks for its logo.
async fn insert(
    state: &AppState,
    user_id: i32,
    new: NewStation,
) -> Result<(Station, bool), AddError> {
    let station = match store::add(&state.db, user_id, &new).await? {
        Added::Existing(station) => return Ok((station, false)),
        Added::Full => return Err(AddError::Invalid(LIST_FULL.into())),
        Added::Created(station) => station,
    };
    // The logo is a nicety: whatever goes wrong with it, the station stays added. For a
    // directory station it comes from where the hit list got it, so a logo that was just shown
    // is not fetched twice; a station added by its address has its homepage's icon, if it
    // names a homepage.
    let logo = match (&station.directory_id, &station.homepage) {
        (Some(directory_id), _) => SearchLogos::get(state, directory_id).await,
        (None, Some(homepage)) => SearchLogos::of_homepage(state, homepage).await,
        (None, None) => None,
    };
    let Some(logo) = logo else {
        return Ok((station, true));
    };
    let with_logo = logos::attach(state, station.id, &logo.original).await?;
    Ok((with_logo.unwrap_or(station), true))
}

fn from_directory(found: DirectoryStation, probed: Probed) -> NewStation {
    NewStation {
        name: found.name.chars().take(100).collect(),
        source_url: found.stream_url,
        codec: found
            .codec
            .or_else(|| codec_of(&probed.content_type).map(str::to_owned)),
        bitrate: found.bitrate.or(probed.bitrate),
        stream_url: probed.stream_url,
        content_type: Some(probed.content_type),
        directory_id: Some(found.directory_id),
        homepage: found.homepage,
        country: found.country,
        tags: found.tags,
    }
}

/// The directory's station `directory_id` in the user's list, with its data and logo. The
/// station and whether it is new: one the user has already comes back untouched, without a
/// question to anybody.
pub async fn add_from_directory(
    state: &AppState,
    user_id: i32,
    directory_id: &str,
) -> Result<(Station, bool), AddError> {
    if !state.settings.get().radio_directory_on() {
        return Err(AddError::DirectoryOff);
    }
    let directory_id = directory_id.trim().to_ascii_lowercase();
    if let Some(station) = store::by_directory_id(&state.db, user_id, &directory_id).await? {
        return Ok((station, false));
    }
    let found = directory::lookup(
        &state.http,
        &state.config.radio_directory_url,
        &directory_id,
    )
    .await
    .map_err(|e| AddError::Directory(e.0))?
    .ok_or(AddError::UnknownStation)?;
    if found.hls {
        return Err(AddError::Invalid(probe::HLS_UNSUPPORTED.into()));
    }
    // The directory's last check may be hours old; what goes into the list has answered now.
    let probed = probe::probe(state, &found.stream_url)
        .await
        .map_err(|e| AddError::Invalid(e.into()))?;
    // What the directory just said about logo and homepage is the newest there is.
    state.radio_logos.remember(std::slice::from_ref(&found));
    let mut new = from_directory(found, probed);
    new.directory_id = Some(directory_id);
    insert(state, user_id, new).await
}

/// A station by its address, under the name the user gave it.
pub async fn add_by_address(
    state: &AppState,
    user_id: i32,
    stream_url: &str,
    name: &str,
    homepage: Option<&str>,
) -> Result<(Station, bool), AddError> {
    let name = valid_name(name).ok_or_else(|| AddError::Invalid(NAME_INVALID.into()))?;
    let homepage = match homepage.map(str::trim).filter(|page| !page.is_empty()) {
        None => None,
        Some(page) => Some(
            probe::parse_url(page)
                .map(|_| page.to_owned())
                .map_err(|_| AddError::Invalid(HOMEPAGE_INVALID.into()))?,
        ),
    };
    let entered = probe::parse_url(stream_url)
        .map_err(|e| AddError::Invalid(e.into()))?
        .to_string();
    if let Some(station) = store::by_url(&state.db, user_id, &entered).await? {
        return Ok((station, false));
    }
    let probed = probe::probe(state, &entered)
        .await
        .map_err(|e| AddError::Invalid(e.into()))?;
    let new = NewStation {
        name: name.to_owned(),
        source_url: entered,
        codec: codec_of(&probed.content_type).map(str::to_owned),
        bitrate: probed.bitrate,
        stream_url: probed.stream_url,
        content_type: Some(probed.content_type),
        homepage,
        ..NewStation::default()
    };
    insert(state, user_id, new).await
}
