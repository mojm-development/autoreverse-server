//! Internet radio: public stations a user keeps in a list of their own and plays through this
//! server.
//!
//! Not to be confused with `library::radio` (`GET /radio`), the "Titel-Radio" that picks similar
//! tracks out of the library. Stations are found in a public directory (`directory`), checked
//! before they are stored (`probe`), kept per user (`store`) and played by passing the sender's
//! stream on (`live`), with the ICY titles read out of it on the way (`icy`). Both go to the
//! sender through `fetch`, which also speaks to old SHOUTcast v1 senders (`icy_v1`). Their logos come
//! from the directory or, failing that, from the station's own web page (`logos`,
//! `search_logos`, `site_icon`).
//!
//! Listening to a station is not booked anywhere: `listen_events` and `progress` are about
//! items of the library, and a live stream is none — it has no position to come back to and
//! nothing the statistics or the suggestions could count it under.

pub mod add;
pub mod directory;
pub mod fetch;
pub mod icy;
pub mod icy_v1;
pub mod live;
pub mod logos;
pub mod probe;
pub mod search_logos;
pub mod site_icon;
pub mod store;

pub use live::Hub;
pub use search_logos::SearchLogos;
