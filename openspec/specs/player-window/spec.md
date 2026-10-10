# player-window Specification

## Purpose
TBD - created by archiving change classic-ui. Update Purpose after archive.
## Requirements
### Requirement: Time display
The main section SHALL show the current time in an LCD-style display derived from the playback clock, toggling between elapsed and remaining on click.

#### Scenario: Toggle remaining
- **WHEN** the user clicks the time display
- **THEN** it shows remaining time prefixed with a minus sign

### Requirement: Track info display
The main section SHALL show a scrolling "N. (catno) Artist: Title (T BPM) (m:ss)" line, bitrate (kbps), sample rate (kHz), and mono/stereo indicators. The name SHALL follow the playlist's display format. For an entry from Discogs, the line SHALL continue with its side, year and a for-sale summary: "K for sale from ‹lowest price›", "none for sale", or nothing when the numbers aren't known. The catalog number SHALL NOT be repeated in that continuation.

#### Scenario: Long title scrolls
- **WHEN** the title text exceeds the display width
- **THEN** it scrolls horizontally

#### Scenario: Discogs details
- **WHEN** entry 3, "Nightcraft" / "Glasshouse" (6:12) at 124 BPM, plays from side A1 of catalog number LT-012 (1994), with 6 copies for sale from €9.00
- **THEN** the line reads "3. (LT-012) Nightcraft: Glasshouse (124 BPM) (6:12) · A1 · 1994 · 6 for sale from €9.00", in the skin's capitals

### Requirement: Transport and sliders
The main section SHALL provide previous, play, pause, stop, next, open, shuffle, and repeat buttons, a seek bar, a volume slider, and a waveform button, all wired to the Engine or the view. The waveform button SHALL sit where the balance slider was, SHALL show or hide the waveform section exactly as `W` does, and SHALL be drawn lit while the waveform section shows. The main section SHALL NOT offer a balance control, and playback SHALL always be centred.

#### Scenario: Drag seek bar
- **WHEN** the user drags the seek bar and releases at 75%
- **THEN** the Engine seeks to 75% of the track duration on release

#### Scenario: Volume change is immediate
- **WHEN** the volume slider is moved
- **THEN** the loudness change is audible within 20 ms

#### Scenario: Waveform button
- **WHEN** the waveform section is hidden and the user clicks the waveform button
- **THEN** the waveform section appears, the button is drawn lit, and the setting is remembered across launches

#### Scenario: Saved balance reset
- **WHEN** the app is launched with a settings file from an older version that holds a balance of -0.6
- **THEN** playback is centred

### Requirement: Mini visualizer
The main section SHALL show a 19-bar spectrum analyzer with peak caps (or an oscilloscope, toggled by click), aligned to the audible position from the playback clock.

#### Scenario: Spectrum tracks audio
- **WHEN** a kick drum is heard
- **THEN** the low bars rise in the same displayed frame (within one UI refresh)

### Requirement: Keyboard shortcuts
The system SHALL support Z (previous), X (play), C (pause), V (stop), B (next), ←/→ (seek ∓5 s), ↑/↓ (volume while the player has focus or in fullscreen; move the playlist cursor while the playlist has focus), Tab (switch focus between the player and the playlist), P (show the playing entry in the playlist), Shift+P (maximize the playlist, or restore it), Space (open or close the record under the playlist cursor while the playlist has focus), F (fullscreen visuals), H or F1 (shortcuts help), W (waveform section), S (spectrogram window), [ and ] (previous/next section), Shift+] (next energy rise), L (section loop), Shift+L (4/8/16-bar loop), Y (add to the wantlist, or remove from it), N (pass), I (open the for-sale page), and Cmd+V, or Ctrl+V on Linux and Windows (paste a Discogs page).

#### Scenario: Classic keys
- **WHEN** the user presses B during playback
- **THEN** the next track starts

#### Scenario: Arrow keys with the player focused
- **WHEN** the player has focus and the user presses ↑
- **THEN** the volume rises by 5%, and the playlist cursor doesn't move

#### Scenario: Arrow keys with the playlist focused
- **WHEN** the playlist has focus and the user presses ↓
- **THEN** the playlist cursor moves down one entry, and the volume doesn't change

#### Scenario: Maximize key
- **WHEN** the user presses Shift+P, and then Shift+P again
- **THEN** the playlist is maximized, and then restored

#### Scenario: Structure keys
- **WHEN** the user presses ] during playback of an analyzed track
- **THEN** playback moves to the next section on the next downbeat

#### Scenario: Spectrogram key
- **WHEN** the user presses S in the player window
- **THEN** the spectrogram window opens

#### Scenario: Dig keys
- **WHEN** the user presses N while a track from Discogs plays
- **THEN** the track is marked passed and the next track starts

#### Scenario: Paste a page
- **WHEN** the user presses Cmd+V with a Discogs label address on the clipboard
- **THEN** the label's tracks are added to the shown crate

#### Scenario: No group key
- **WHEN** the user presses Shift+G with a flat crate shown
- **THEN** nothing happens, and the help (H) does not list Shift+G

### Requirement: Low idle cost
The UI SHALL consume near-zero CPU when idle and SHALL NOT repaint while minimized or occluded.

#### Scenario: Idle stopped
- **WHEN** nothing is playing and there is no input for 10 s
- **THEN** the UI performs no repaints

### Requirement: Fast launch
The app SHALL show an interactive window within 300 ms of launch on macOS.

#### Scenario: Cold launch
- **WHEN** the app is launched with a persisted 500-entry playlist
- **THEN** the window is interactive within 300 ms and durations fill in afterwards

### Requirement: Shortcuts help
`H` or `F1` SHALL open a panel listing every keyboard and mouse shortcut, grouped by area, in the player window and in fullscreen; `H`, `F1`, `Esc` or the panel's close button SHALL close it, and `Esc` SHALL close the panel before it leaves fullscreen.

#### Scenario: Open help
- **WHEN** the user presses H in the player window
- **THEN** a panel lists the playback, structure, player window, fullscreen visuals and mouse shortcuts

#### Scenario: Esc in fullscreen
- **WHEN** the help panel is open in fullscreen and the user presses Esc
- **THEN** the panel closes and fullscreen stays on

### Requirement: Section focus
The player side (main, waveform and EQ sections) and the playlist SHALL each take keyboard focus when clicked, and Tab SHALL switch focus between them. The app SHALL start with the player focused. The focused side's title bars SHALL be drawn lit and the other side's dimmed, in the same frame as the click. Hovering SHALL NOT change focus. While a text field has keyboard focus, no shortcut SHALL act.

#### Scenario: Click the playlist
- **WHEN** the player has focus and the user clicks an entry in the playlist
- **THEN** the playlist's title bar is drawn lit, the main and EQ title bars dimmed, and ↑/↓ now move the playlist cursor

#### Scenario: Tab
- **WHEN** the playlist has focus and the user presses Tab
- **THEN** the player has focus and ↑ raises the volume

#### Scenario: Launch
- **WHEN** the app starts
- **THEN** the player has focus

### Requirement: Options menu
Right-clicking (or Control-clicking on macOS) anywhere on the main window, or on the mini player of a maximized playlist, that is not a control SHALL open the Options menu: Double size (or Classic size), Spectrogram, and, where digging is available, Discogs… and Browser…. The controls SHALL keep their own clicks. Hovering that area SHALL show a tooltip saying that a right-click opens the options. The playlist footer's gear button SHALL open the same menu, so it can be found without knowing about the right-click. Opening the menu SHALL NOT affect playback.

#### Scenario: Open the options
- **WHEN** the user right-clicks the main window's track-info area
- **THEN** the Options menu opens with Double size, Spectrogram, Discogs… and Browser…

#### Scenario: Controls keep their clicks
- **WHEN** the user right-clicks the volume slider
- **THEN** the Options menu doesn't open

#### Scenario: Maximized
- **WHEN** the playlist is maximized and the user right-clicks the mini player's title
- **THEN** the Options menu opens

### Requirement: Verdict flash
When a verdict changes at least one record, the title line SHALL show, for 1.5 s and without scrolling, centred: "WANTED" after adding to the wantlist, "UNWANTED" after removing from it, "PASS" after a pass and "OWNED" after adding to the collection. When more than one record changed, the count SHALL follow ("WANTED 3"). A verdict that changes nothing (Y on an owned record, N on a wanted one, a key while stopped) SHALL show no flash. A newer flash SHALL replace an older one, and when the flash ends the normal title line SHALL come back.

#### Scenario: Want
- **WHEN** the user presses Y while a record not on the wantlist plays
- **THEN** the title line shows "WANTED" for 1.5 s, then the track line again

#### Scenario: Pass and next track
- **WHEN** the user presses N while track 5 plays
- **THEN** the title line shows "PASS" for 1.5 s while track 6 starts, then shows track 6's line

#### Scenario: Several records
- **WHEN** the user adds 3 selected records to the wantlist from the right-click menu
- **THEN** the title line shows "WANTED 3"

#### Scenario: Nothing changed
- **WHEN** the user presses N on a track whose record is on the wantlist
- **THEN** no flash is shown

