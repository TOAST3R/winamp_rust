## Context

- Seller crates are the model: `dig::sellers::SellerList` (`<config>/dig/sellers.ron`) keeps the list and per-seller state, `CrateInfo.seller` ties a crate to a seller (it survives a rename), and the sidebar draws TOP SELLERS as a foldable group. Click / double-click handling lives in `ui/src/app/sellers.rs`.
- A collection page can already be sent: `PageKind::Collection(user)` lists `/users/‹user›/collection/folders/0/releases` and expands each record (`dig/src/discogs/expand.rs`). The user's own collection crate uses it, and Refresh collection diffs the crate against the owned releases and sends the missing ones.
- `GET /users/‹you›/friends` with the token returns `{pagination, friends: [{user: {username, name, avatar_url, …}, added}]}`. The developer docs don't list it. Checked on 2026-10-11: 12 friends; collections of 7 readable, 5 answered 403.
- `maximized-deck` (PR #42) adds the Refresh collection progress window with Stop and `Command::StopJobs`. This change reuses them, so it builds on that branch being merged.

## Goals / Non-Goals

**Goals:**
- FRIENDS in the sidebar, filled from Discogs, with no way to edit it in the app.
- Digging a friend's collection with the machinery collection pages already use.
- Fail soft if the undocumented endpoint changes.

**Non-Goals:**
- Friends' wantlists, lists or inventories.
- Adding, removing or reordering friends; avatars in the sidebar.
- Seller features in friend crates (prices, SOLD, Narrow down, copy rows).
- A friend crate's OWNED badge counting the friend's ownership: OWNED stays about the user.

## Decisions

**1. A `FriendList` beside `SellerList`.** `dig::friends::FriendList` is saved as `<config>/dig/friends.ron` and holds `read_at`, `failed: Option<(u64, String)>`, `folded`, and `friends: Vec<Friend { username, private: bool, dug_at: Option<u64> }>` in Discogs' order. It's a plain serde file loaded at launch: a missing or broken file loads empty, as `sellers.ron` does.
   - *Alternative:* store friends in the crate index. Rejected, because the list's fetch state isn't about crates, and sellers already set the pattern.

**2. Friends are tied to crates by id in `friends.ron`**, as sellers are (`Seller.crate_id`), so a renamed crate is still recognised and a friend who leaves just drops out of the list, which leaves their crate an ordinary one. A friend's crate is set grouped when it's made.

**3. Reading the list is a background step in the worker.** `Command::ReadFriends` runs at a moment with no send (like the first Top Sellers list): it reads every page of `/users/‹you›/friends?per_page=100`, then checks each friend's collection with one `per_page=1` request (only a 403 means private; only being offline fails the read), and answers `Event::Friends(Result<Vec<(String, bool)>, ApiError>)`. The UI sends it at launch once the window is interactive, when `read_at` is 7 days old or missing (or 1 day after a failure), and when a token is first saved.

**4. Merging a new list in the UI.** For each username, the UI creates a friend crate if none is tied to it, keeps the order, and updates `private`. A crate tied to a username that's no longer listed gets `friend = None`, so it becomes an ordinary crate and keeps its name and records. On `Err`, nothing changes except `failed`, which the heading's tooltip shows.

**5. Digging is a collection-page send.** A double-click on a never-dug, non-private friend sends `Page::new(PageKind::Collection(username))` into the crate with `SendMode::Crate`, the way the collection crate is filled. When the send's listing fails with 403, the friend is marked private, the dimmed crate is saved, and the main window says so. `dug_at` is set when the send is accepted.

**6. Refresh friend reads the collection, then follows it.** The worker reads the friend's whole collection a page at a time with the `collection::Syncer` (`Command::ReadFriendCollection`, answered with `FriendProgress` and `FriendCollection`). The UI then makes the crate hold exactly those releases with the same helper Refresh collection uses (`follow_releases`): records gone leave, and missing ones are sent one by one, or as the collection page when there are many. The refresh window shows "Reading ‹name›'s collection… page k of n", then the fill, with Stop (`CancelCollectionSync`, `StopJobs`).
   - *Alternative:* send the collection page again and remove what wasn't listed. Rejected, because a page sent again lists every record as a placeholder, and the crate holds them until each one's details arrive.

**7. No new request rate.** Every request goes through the existing client and its rate limiter. A friend's first dig costs the same as sending their collection page by hand (about one request per 100 records listed, then one per record).

## Risks / Trade-offs

- [The endpoint is undocumented and may change] → The list fails soft (decision 4), and the heading's tooltip says why.
- [Big collections take long to dig (1,181 records is about 20 minutes)] → Only on double-click; the details come nearest the playhead first, as for every send; the progress window shows it and Stop ends it.
- [A friend's collection can include records the user owns many of] → OWNED marks them, so they're easy to skip.
- [Depends on `maximized-deck` landing first] → Apply this change on a branch from `main` after PR #42 is merged.

## Migration Plan

New file `friends.ron`, and a new optional field on `CrateInfo` with a serde default. Older indexes load unchanged. Rollback is a revert; friend crates would then show as ordinary crates.

## Open Questions

- Should a friend's crate show their display name ("Peter Furlan") as well as the username? Left out for now; the username is what Discogs links use.
