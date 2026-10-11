//! Digging: turning a Discogs page into a crate of previews, ready to navigate before they play.
//!
//! Everything here runs on low-priority worker threads and talks to the UI over channels, so
//! nothing in this crate can ever touch the playback path:
//! - [`intake`]: one worker makes every Discogs request in turn (so the rate limit is simple);
//! - [`preview`]: fetches clips with the user's own yt-dlp, a few tracks ahead;
//! - [`prepare`]: analyzes downloaded previews so they open with their waveform and sections;
//! - [`bridge`]: a loopback server through which a paired browser extension sends pages;
//! - [`cover`]: record covers for the entry tooltip, from Discogs' image host.
//! - [`bandcamp`]: Bandcamp pages, checked and read through yt-dlp.
//!
//! Native only: the web build leaves this crate out.

pub mod bandcamp;
pub mod bridge;
pub mod browser;
pub mod clock;
pub mod collection;
pub mod config;
pub mod cover;
pub mod discogs;
pub mod friends;
pub mod intake;
pub mod jobs;
pub mod memory;
pub mod prepare;
pub mod preview;
pub mod sellers;

/// The player's name, as the browser extension shows it ("Play in ‹name›"). A placeholder
/// until the rebrand.
pub const APP_NAME: &str = "Diggr";

/// Seconds since the Unix epoch (the timestamps stored in files).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A fresh, empty temp folder for a test.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> platform::testing::TestDir {
    platform::testing::TestDir::new(&format!("dig-{name}"))
}
