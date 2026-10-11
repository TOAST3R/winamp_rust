## 1. Friends list in `dig`

- [x] 1.1 `dig::friends`: `FriendList` / `Friend` (username, private, dug_at), `read_at`, `failed`, `folded`; load (missing or broken → empty) and save `<config>/dig/friends.ron`; `due(now)` (7 days, or 1 day after a failure); unit tests with `TestDir`
- [x] 1.2 Request: read every page of `/users/‹you›/friends?per_page=100` into usernames in order; a fixture for the fake transport; unit tests (two pages, 404, missing `friends` key)
- [x] 1.3 Worker: `Command::ReadFriends` as a background step (after the first Top Sellers list); one `per_page=1` collection request per friend (403 = private); `Event::Friends(Result<Vec<(String, bool)>, ApiError>)`; intake test

## 2. Friend crates in the UI

- [x] 2.1 Friend crates tied by crate id in `friends.ron` (like sellers), so a rename keeps them; grouped when made
- [x] 2.2 Load `friends.ron` at launch; send `ReadFriends` once interactive when due, and when a token is first saved; merge an `Ok` list (create crates, keep order, update private, untie friends no longer listed); on `Err` keep the list and record `failed`
- [x] 2.3 Sidebar: FRIENDS heading under TOP SELLERS with ⏵/⏷, count and fold (saved); friend rows with record counts; private ones dimmed, no count, tooltip "Private collection"; heading tooltip with last read and the failure; hidden without a token or friends

## 3. Dig and refresh

- [x] 3.1 Click shows (no request); empty never-dug crate says "Double-click to dig ‹username›'s collection"; double-click sends `PageKind::Collection(username)` into the crate (never-dug, not private), only shows a dug one, says a private one is private, and opens Connect without a token
- [x] 3.2 A 403 while listing a friend's collection marks them private, dims the crate and says so
- [x] 3.3 Refresh friend on the crate's right-click (sidebar and title-bar menu): read their collection a page at a time (`ReadFriendCollection`), then make the crate follow it (`follow_releases`: gone leave, missing are sent); "Refreshing…" disabled while it runs; summary "‹name›: 3 new, 1 gone" / "up to date"; failures change nothing and say why
- [x] 3.4 The refresh window shows a friend's dig and refresh (fill phase) with Stop (`StopJobs`, listed placeholders removed)

## 4. Tests

- [x] 4.1 Headless: a saved token reads 12 friends into FRIENDS with one request; a read within 7 days sends nothing; a read 8 days later adds a new friend and unties a missing one (its crate and records stay, at the top)
- [x] 4.2 Headless: click sends nothing; double-click fills a friend's crate grouped with OWNED on owned records; a 403 marks private and later double-clicks send nothing; a weekly read with a 200 clears private
- [x] 4.3 Headless: Refresh friend brings 3 new and removes 1 gone, says so; Stop keeps what arrived; a failed friends read keeps the list and the heading tooltip says why
- [x] 4.4 Headless: digging a friend while a track plays keeps playback going (no underruns)

## 5. Docs and checks

- [x] 5.1 `README.md`: a Friends section (where the list comes from, weekly read, dig, refresh, private, leaving); `friends.ron` in the file locations; the test count
- [x] 5.2 `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cargo check -p audio -p platform --target wasm32-unknown-unknown` all clean
- [x] 5.3 Run the app with your token: FRIENDS shows your 12 friends with the 5 private ones dimmed, dig javimaxilo, and refresh it
