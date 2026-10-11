//! Friends: the user's Discogs friends, each with a crate of their collection, kept in
//! `<config>/dig/friends.ron`.
//!
//! The list comes only from Discogs (`/users/‹you›/friends`, which the developer docs don't
//! list), read at most once a week; there is no adding or removing here. A read that fails
//! keeps the last list, so the group never empties because the endpoint changed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discogs::client::{ApiError, Client, path_segment};

pub const FILE: &str = "friends.ron";
/// The list is read again once its last read is this old.
pub const WEEK_SECS: u64 = 7 * 24 * 60 * 60;
/// After a failed read, the next try waits this long.
pub const RETRY_SECS: u64 = 24 * 60 * 60;
const PER_PAGE: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Friend {
    pub username: String,
    /// The crate holding their collection.
    pub crate_id: u64,
    /// Discogs answered 403 to their collection.
    #[serde(default)]
    pub private: bool,
    /// Seconds since the Unix epoch; `None` until the first dig.
    #[serde(default)]
    pub dug_at: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FriendList {
    /// When the list was last read from Discogs.
    pub read_at: Option<u64>,
    /// When the last read failed, and why (cleared by a read that works).
    pub failed: Option<(u64, String)>,
    /// FRIENDS is folded in the sidebar.
    pub folded: bool,
    /// In Discogs' order.
    pub friends: Vec<Friend>,
}

impl FriendList {
    pub fn path(config: &Path) -> PathBuf {
        crate::config::dir(config).join(FILE)
    }

    /// Missing or broken: an empty list, never read (a broken file never blocks launch).
    pub fn load(config: &Path) -> Self {
        std::fs::read_to_string(Self::path(config))
            .ok()
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config: &Path) -> std::io::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(std::io::Error::other)?;
        crate::config::write_atomic(&Self::path(config), text.as_bytes(), false)
    }

    /// Whether the list is to be read now: never read, a week old, or a day after a failure.
    pub fn due(&self, now: u64) -> bool {
        if let Some((at, _)) = &self.failed
            && now.saturating_sub(*at) < RETRY_SECS
        {
            return false;
        }
        self.read_at
            .is_none_or(|t| now.saturating_sub(t) >= WEEK_SECS)
    }

    pub fn by_crate(&self, crate_id: u64) -> Option<&Friend> {
        self.friends.iter().find(|f| f.crate_id == crate_id)
    }

    pub fn by_crate_mut(&mut self, crate_id: u64) -> Option<&mut Friend> {
        self.friends.iter_mut().find(|f| f.crate_id == crate_id)
    }

    pub fn find(&self, username: &str) -> Option<&Friend> {
        self.friends
            .iter()
            .find(|f| f.username.eq_ignore_ascii_case(username))
    }
}

/// The user's friends from Discogs, in its order (every page).
pub fn read(client: &mut Client, user: &str) -> Result<Vec<String>, ApiError> {
    let mut names = Vec::new();
    let mut n = 1;
    loop {
        let path = format!(
            "/users/{}/friends?page={n}&per_page={PER_PAGE}",
            path_segment(user)
        );
        let v = client.get_json(&path)?;
        let page = v["friends"]
            .as_array()
            .ok_or_else(|| ApiError::Other(format!("{path}: no friends list")))?;
        names.extend(
            page.iter()
                .filter_map(|f| f["user"]["username"].as_str().map(str::to_owned)),
        );
        if n >= v["pagination"]["pages"].as_u64().unwrap_or(1) as u32 || page.is_empty() {
            return Ok(names);
        }
        n += 1;
    }
}

/// Whether `user`'s collection is private now, with one small request. Only a 403 says so;
/// any other answer leaves them digging (and a dig says what's wrong), and only being
/// offline fails the read.
pub fn is_private(client: &mut Client, user: &str) -> Result<bool, ApiError> {
    let path = format!(
        "/users/{}/collection/folders/0/releases?page=1&per_page=1",
        path_segment(user)
    );
    match client.get_json(&path) {
        Err(ApiError::Private) => Ok(true),
        Err(ApiError::Offline) => Err(ApiError::Offline),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discogs::transport::FakeTransport;
    use std::sync::Arc;

    fn friend(name: &str, crate_id: u64) -> Friend {
        Friend {
            username: name.into(),
            crate_id,
            private: false,
            dug_at: None,
        }
    }

    #[test]
    fn the_list_survives_a_restart_and_a_broken_file_is_empty() {
        let d = crate::test_dir("friends");
        let mut l = FriendList {
            read_at: Some(1_790_000_000),
            folded: true,
            ..FriendList::default()
        };
        l.friends.push(friend("javimaxilo", 20));
        l.friends.push(Friend {
            private: true,
            ..friend("Waxport", 21)
        });
        l.save(d.path()).unwrap();
        assert_eq!(FriendList::load(d.path()), l);
        std::fs::write(FriendList::path(d.path()), "not ron").unwrap();
        assert_eq!(FriendList::load(d.path()), FriendList::default());
    }

    #[test]
    fn read_weekly_and_a_day_after_a_failure() {
        let now = 1_790_000_000;
        let mut l = FriendList::default();
        assert!(l.due(now), "never read");
        l.read_at = Some(now - 3 * 24 * 3600);
        assert!(!l.due(now));
        l.read_at = Some(now - 8 * 24 * 3600);
        assert!(l.due(now));
        l.failed = Some((now - 3600, "Discogs offline".into()));
        assert!(!l.due(now), "not again within a day of a failure");
        l.failed = Some((now - 25 * 3600, "Discogs offline".into()));
        assert!(l.due(now));
    }

    fn client(t: &Arc<FakeTransport>) -> Client {
        Client::new(
            t.clone(),
            Arc::new(crate::clock::FakeClock::default()),
            Some("tok".into()),
            crate::discogs::cache::DiskCache::new(None),
        )
    }

    #[test]
    fn every_page_of_friends_is_read_in_order() {
        let t = Arc::new(FakeTransport::new());
        for (n, names) in [(1, ["a", "b"]), (2, ["c", "d"])] {
            let friends: Vec<_> = names
                .iter()
                .map(|u| serde_json::json!({"user": {"username": u}, "added": "2025"}))
                .collect();
            t.route(
                format!("/users/me/friends?page={n}&per_page=100"),
                200,
                serde_json::json!({"pagination": {"page": n, "pages": 2}, "friends": friends})
                    .to_string(),
            );
        }
        assert_eq!(read(&mut client(&t), "me").unwrap(), ["a", "b", "c", "d"]);
        // An answer without a friends list, or none at all, is an error: the list stays.
        let t = Arc::new(FakeTransport::new());
        t.route("/users/me/friends?page=1&per_page=100", 200, "{}");
        assert!(read(&mut client(&t), "me").is_err());
        let t = Arc::new(FakeTransport::new());
        assert_eq!(read(&mut client(&t), "me"), Err(ApiError::NotFound));
    }

    #[test]
    fn a_private_collection_answers_403() {
        let t = Arc::new(FakeTransport::new());
        let path = "/users/Waxport/collection/folders/0/releases?page=1&per_page=1";
        t.route(path, 403, "{}");
        assert_eq!(is_private(&mut client(&t), "Waxport"), Ok(true));
        t.route(path, 200, "{}");
        assert_eq!(is_private(&mut client(&t), "Waxport"), Ok(false));
        // A user Discogs doesn't know any more isn't private: their dig will say why.
        assert_eq!(is_private(&mut client(&t), "gone"), Ok(false));
    }
}
