//! FRIENDS: a crate per Discogs friend, holding their collection. The list comes from
//! Discogs only (see `dig::friends`); a click shows a crate, a double-click digs a new one,
//! and Refresh friend reads their collection again and makes the crate follow it.

use std::collections::HashSet;

use dig::collection::Collection;
use dig::discogs::client::ApiError;
use dig::discogs::url::{Page, PageKind};
use dig::friends::Friend;
use dig::intake::Command;

use super::DiggrApp;
use crate::crates::{CrateId, MAX_NAME};

/// Friend crates are named "Friend: ‹username›".
pub const FRIEND_PREFIX: &str = "Friend: ";

/// "Friend: ‹name›", cut to the crates' name limit.
pub fn friend_crate_name(username: &str) -> String {
    let name = format!("{FRIEND_PREFIX}{username}");
    name.chars().take(MAX_NAME).collect()
}

/// What the sidebar asks of FRIENDS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FriendAction {
    /// Fold or unfold FRIENDS.
    ToggleFold,
    /// A double-click on a friend's crate: dig it the first time.
    Dig(CrateId),
    /// Refresh friend: read their collection again.
    Refresh(CrateId),
}

/// A friend's crate as the sidebar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendRow {
    pub crate_id: CrateId,
    pub dug: bool,
    pub private: bool,
    pub busy: bool,
}

impl DiggrApp {
    /// The friend crates in Discogs' order, and whether FRIENDS is folded (none without a
    /// token).
    pub(super) fn friend_rows(&self) -> (Vec<FriendRow>, bool) {
        let Some(d) = self.dig.as_ref().filter(|d| d.has_token()) else {
            return (Vec::new(), false);
        };
        let rows = d
            .friends
            .friends
            .iter()
            .filter(|f| self.crates.info(f.crate_id).is_some())
            .map(|f| FriendRow {
                crate_id: f.crate_id,
                dug: f.dug_at.is_some(),
                private: f.private,
                busy: self.friend_busy(f.crate_id),
            })
            .collect();
        (rows, d.friends.folded)
    }

    /// What FRIENDS' heading says on hover: when the list was read, and why the last read
    /// failed.
    pub(super) fn friends_tip(&self) -> String {
        let Some(d) = &self.dig else {
            return String::new();
        };
        let mut tip = "Your Discogs friends' collections: click to fold".to_owned();
        if let Some(at) = d.friends.read_at {
            tip += &format!(
                "\nList read {}",
                crate::format::ago(super::digging::now_secs().saturating_sub(at))
            );
        }
        if let Some((_, why)) = &d.friends.failed {
            tip += &format!("\nCouldn't read it again: {why}");
        }
        tip
    }

    /// What an empty friend's crate says instead of how to paste: how to dig it, or that
    /// it's private.
    pub(super) fn friend_hint(&self, c: CrateId) -> Option<Vec<String>> {
        let f = self.friend_of(c)?;
        Some(if f.private {
            vec![format!("{}'S COLLECTION", f.username), "IS PRIVATE".into()]
        } else if f.dug_at.is_some() {
            vec![format!("{}'S COLLECTION", f.username), "IS EMPTY".into()]
        } else {
            vec![
                "DOUBLE-CLICK TO DIG".into(),
                format!("{}'S COLLECTION", f.username),
            ]
        })
    }

    fn friend_busy(&self, c: CrateId) -> bool {
        self.dig
            .as_ref()
            .is_some_and(|d| d.has_job_for(c) || d.friend_refresh.is_some_and(|(t, _)| t == c))
    }

    fn friend_of(&self, c: CrateId) -> Option<Friend> {
        self.dig.as_ref()?.friends.by_crate(c).cloned()
    }

    pub(super) fn friend_act(&mut self, a: FriendAction) {
        match a {
            FriendAction::ToggleFold => {
                let Some(d) = &mut self.dig else { return };
                d.friends.folded = !d.friends.folded;
                let e = d.save_friends();
                self.dig_notify(e);
            }
            FriendAction::Dig(c) => self.friend_dig(c),
            FriendAction::Refresh(c) => self.friend_refresh(c),
        }
    }

    /// A double-click: a private friend says so, a never-dug one is sent, a dug one is shown.
    fn friend_dig(&mut self, c: CrateId) {
        let Some(f) = self.friend_of(c) else { return };
        let Some(d) = &mut self.dig else { return };
        if !d.has_token() {
            d.connect = Some(super::digging::ConnectDialog {
                from_wantlist: false,
            });
            return;
        }
        if f.private {
            return self.notify(format!("{}'s collection is private", f.username));
        }
        if f.dug_at.is_some() {
            return;
        }
        if let Some(e) = d.friends.by_crate_mut(c) {
            e.dug_at = Some(super::digging::now_secs());
        }
        d.refilling = Some((c, false));
        let e = d.save_friends();
        self.dig_notify(e);
        self.crates.set_grouped(c, true);
        self.dig_fill(Page::new(PageKind::Collection(f.username)), c);
    }

    /// Refresh friend: their whole collection is read again, then the crate follows it.
    fn friend_refresh(&mut self, c: CrateId) {
        let Some(f) = self.friend_of(c) else { return };
        if self.friend_busy(c) || f.private {
            return;
        }
        let Some(d) = &mut self.dig else { return };
        d.friend_refresh = Some((c, None));
        d.send_cmd(Command::ReadFriendCollection {
            target: c,
            user: f.username,
        });
    }

    /// How far a friend's collection read got.
    pub(super) fn friend_progress(&mut self, c: CrateId, read: u32, pages: u32) {
        if let Some(d) = &mut self.dig
            && let Some((t, p)) = &mut d.friend_refresh
            && *t == c
        {
            *p = Some((read, pages));
        }
    }

    /// A friend's collection was read whole: the crate follows it, and the main window says
    /// what changed. A private one is marked; any other failure changes nothing.
    pub(super) fn friend_collection(
        &mut self,
        c: CrateId,
        result: Result<Box<Collection>, ApiError>,
    ) {
        let Some(d) = &mut self.dig else { return };
        if d.friend_refresh.is_none_or(|(t, _)| t != c) {
            return; // stopped
        }
        d.friend_refresh = None;
        let Some(f) = self.friend_of(c) else { return };
        let releases: HashSet<u64> = match result {
            Ok(col) => col.releases.keys().copied().collect(),
            Err(ApiError::Private) => return self.friend_private(c),
            Err(e) => {
                return self.notify(format!("{}: refresh failed: {}", f.username, e.message()));
            }
        };
        if let Some((new, gone)) = self.follow_releases(c, &releases, Some(f.username.clone())) {
            let line = match (new, gone) {
                (0, 0) => format!("{}: up to date", f.username),
                (n, 0) => format!("{}: {n} new", f.username),
                (0, g) => format!("{}: {g} gone", f.username),
                (n, g) => format!("{}: {n} new, {g} gone", f.username),
            };
            self.notify(line);
        }
    }

    /// Discogs answered 403 to a friend's collection: the crate is dimmed from now on.
    pub(super) fn friend_private(&mut self, c: CrateId) {
        let Some(d) = &mut self.dig else { return };
        let Some(f) = d.friends.by_crate_mut(c) else {
            return;
        };
        f.private = true;
        let name = f.username.clone();
        let e = d.save_friends();
        self.dig_notify(e);
        self.notify(format!("{name}'s collection is private"));
    }

    /// The friends list arrived: a crate for each new friend, in Discogs' order; friends no
    /// longer listed leave the group (their crates stay, as ordinary ones). A failure keeps
    /// the list and remembers why.
    pub(super) fn dig_friends(&mut self, result: Result<Vec<(String, bool)>, ApiError>) {
        let now = super::digging::now_secs();
        let list = match result {
            Ok(list) => list,
            Err(e) => {
                let Some(d) = &mut self.dig else { return };
                d.friends.failed = Some((now, e.message()));
                let e = d.save_friends();
                return self.dig_notify(e);
            }
        };
        let Some(d) = &self.dig else { return };
        let old = d.friends.friends.clone();
        let mut friends = Vec::new();
        for (username, private) in list {
            let kept = old
                .iter()
                .find(|f| {
                    f.username.eq_ignore_ascii_case(&username)
                        && self.crates.info(f.crate_id).is_some()
                })
                .cloned();
            let friend = match kept {
                Some(f) => Friend { private, ..f },
                None => {
                    let name = friend_crate_name(&username);
                    let Some(id) = self
                        .crates
                        .find(&name)
                        .or_else(|| self.crates.create(&name).ok())
                    else {
                        continue;
                    };
                    self.crates.set_grouped(id, true);
                    Friend {
                        username,
                        crate_id: id,
                        private,
                        dug_at: None,
                    }
                }
            };
            friends.push(friend);
        }
        let Some(d) = &mut self.dig else { return };
        d.friends.friends = friends;
        d.friends.read_at = Some(now);
        d.friends.failed = None;
        let e = d.save_friends();
        self.dig_notify(e);
    }
}
