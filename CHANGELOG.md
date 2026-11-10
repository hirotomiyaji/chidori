# Changelog

All notable changes are listed here. Versions follow [SemVer](https://semver.org/); the workspace `Cargo.toml` holds the current version.

## [Unreleased]

### Fixed

- 設定's sections all show: in a narrow window the chips wrap onto a second line instead of running off its edge, which hid 出力.
- 後で on the recovery dialog puts off every interrupted recording at once instead of one per press, and the dialog says how many more are waiting.
- The window buttons stay inside a narrow window, and inside the default one in English: 「Chidori」 makes way first, then the tabs move closer together.
- 履歴's project chips scroll sideways when there are more than fit.
- The transcript scrolls up behind the session card instead of being cut along a straight line under it, and the card's ▶ sits in the middle of its circle.

## [0.8.2] - 2026-11-10

### Changed

- Menus are drawn in Chidori's own look instead of Windows': the ⋯ menus, コピー ▾, the camera's and the source's open at the pointer, with arrow keys, Enter and Esc. One too tall for the compact window still opens as Windows' menu.
- A right click opens the menu of what is under it: a session in 履歴, the session card, a line (with 「この行をコピー」), a screenshot, the title bar (常に手前に表示, 縮小, 最小化, 閉じる, in place of Windows' menu) and text boxes (切り取り, コピー, 貼り付け, すべて選択, in Japanese).

### Fixed

- Buttons look right under the pointer: コピー and 開始 no longer turn grey with their words fading, white buttons no longer vanish into the background, and the title bar's tabs, 設定's sections, the source choices and other buttons that showed nothing now light up.
- A double click or a second hotkey press on 開始 no longer starts a second session, and a second press on 停止 does nothing.
- A session left by a crash stays recoverable in 履歴, even after a continued recording or a rename.
- A past session's sound stops when you leave it or a recording starts.
- Finishing no longer waits forever after a model failed to load; it saves what it has.
- A ★ on speech too short to make a line lands on the line said then.
- Smaller fixes: clock times when continuing a file's session, continuing after a crash mid-write, settings changes overwriting a recording's source, and importing a model during its download.

## [0.8.1] - 2026-11-08

### Added

- A Mac version, in early testing: `Chidori_…_aarch64.dmg` for Apple Silicon Macs on macOS 14.6 or later. It records the microphone and all system audio and transcribes on the Mac's GPU; recording a single app, and transcribing files other than WAV and Opus, come later. It isn't notarized yet, so the first launch goes through System Settings → Privacy & Security → Open Anyway, and its update banner opens the download page.

## [0.7.0] - 2026-11-08

### Changed

- A new look. Greyscale with colour only for who is speaking (相手 coral, 自分 teal) and the red record dot, and no shadows. Recording shows an ink card with ON AIR, the title, the source, the timer and the live sound, over the transcript as speech bubbles: 相手 on the left, 自分 on the right. A finished session's card turns white with its play button and sound, and コピー, 書き出し and ⋯ sit in the bar below. The source is picked from two panels (相手 and 自分) on the start screen. 履歴 has a search pill, project chips and one panel per day; 設定 has section chips instead of a list. Tabs, chips, the chosen source, switches, new lines, toasts and the recording card move with one springy motion, and stay still when Windows' animation effects are off.
- Chidori is now drawn natively instead of in a web view, and is lighter: about 40 MB less memory while it waits, and about half the CPU while it draws the line during a recording. It looks and works as before.
- Updates install over the previous version as before; settings, models and recordings stay where they were.

## [0.5.1] - 2026-10-25

0.5.0 was built but never published. Its changes are all here.

### Added

- Transcribe an audio or video file you already have: 「ファイルから文字起こし」 under 開始, or drop the file on the window. Anything Windows plays works (MP4, MOV, MKV, WebM, M4A, MP3, WAV, FLAC, WMA), and Opus. It runs as fast as the PC keeps up, shows how far into the file it has got, and saves a session named after the file whose times are places in the file.
- Delete the session you are reading from its ⋯ menu, and copy a session straight from its row in 履歴.

### Changed

- A session has the same ⋯ menu wherever it is shown: open, or as a row in 履歴.
- コピー has a ▾ beside it for 「Agent用にコピー」 and 「Markdownでコピー」, which used to be in ⋯.
- Projects are managed in 履歴: pick a project in the filter and the page becomes that project's, with a ⋯ to change its name and colour or delete it. 設定 › プロジェクト is gone.

## [0.4.1] - 2026-10-19

0.4.0 was built but never published: its sound did not play back. Its changes are all here.

### Added

- The setup wizard's shortcut step sets the shortcuts, or leaves them off.
- While recording, words still on their way show 「文字起こし中…」 with a spinner, so a slow PC no longer looks stuck; 仕上げ中 has a spinner too.

### Changed

- Kikitori is now Chidori. Updating from Kikitori removes the old install and brings your settings, models and recordings along (`Documents\Kikitori` becomes `Documents\Chidori`).
- A new look from the kamon brand: a warm grey ground, the plover crest as the icon, Zen Kaku Gothic New, and Chidori's 朱 for buttons and the voice you hear. 開始 is a round button with the record dot, and errors and warnings come on brick and ochre with their words.
- The tray icon is ink while idle and takes its colour while recording.
- PDFs use the same ink and 朱, with a double rule under the title.
- The window opens expanded until you shrink it.

### Fixed

- When transcription fell behind, echo of 相手 in a long 自分 line could be kept as 自分's words.
- A saved session's sound plays back in the installed app, and each press that cannot open the file says so.

## [0.3.0] - 2026-10-10

### Added

- A minute behind, a banner offers to transcribe after Stop: Whisper stops while recording so a slow PC keeps up, and the speech is written, 相手 and 自分 still apart, when you stop. 今すぐ再開 on the line takes it up again.
- Finishing that takes more than 5 s shows a progress bar.

### Changed

- 「遅れ n秒」 sits on the line, where it fits in both windows (the buttons added in 0.2.0 had squeezed it out), and turns bold from 15 s and yellow from 30 s.
- 設定 › モデル lists the recommended models and the ones you have; the others open from ほかのモデル.

## [0.2.0] - 2026-10-08

### Added

- A star button while recording marks the line being said as important, even before its text arrives; any line's star adds or removes the mark later. Exports and copies start important lines with ★, and the prompt for AI agents explains it.
- Hotkeys for the important mark and the cut.
- Switch the source while recording, without stopping; the transcript notes the switch.
- The recording keeps a header with its title (click to rename) and its source.
- The camera's menu picks what screenshots take: the app's window, a screen, all screens, or any open window.
- Recordings keep their sound (Opus, about 11 MB an hour): play a finished session back from any line, with the line and the session's line following the sound.
- 続きを録音 records more onto a saved session, today's or an old one, keeping its clock times true.
- Projects group recordings under a name and a colour; History filters by project, and Markdown front matter names it.
- A cut button while recording starts a new part of the session. The transcript shows the cut, Markdown exports head each part with `## HH:MM:SS`, and the PDF lists the parts in its outline.

### Changed

- 最新へ jumps straight to the newest line when it is far away, instead of gliding there for seconds in a long session.

## [0.1.0] - 2026-10-05

### Added

- Live transcription with local Whisper (whisper.cpp), on the GPU through Vulkan with CPU fallback. Japanese and English come out as spoken, whichever language is set, never translated.
- Sources: one app's audio (Windows 11), all system audio, and the microphone, labelled 相手 and 自分, with an echo guard for speakers. The source picker shows each app's live sound beside its name while it plays.
- Screenshots by hotkey or button, placed in the transcript where they were taken.
- Sessions saved as Markdown with images, recovered after a crash, and kept in a searchable history grouped by day, with line editing and moving several to the Recycle Bin at once.
- A small window in white, black and one red that follows the Windows light or dark theme, with a pin to keep it on top. A red dot starts a line drawn from the live sound, 相手 above and 自分 below, under a transcript grouped into speaker turns, with pause/resume, stop and screenshot controls. Each finished session keeps the shape of its sound as a timeline to jump through and as its picture in the history.
- Copy as plain text, Markdown or a prompt for AI agents; export, each saved where you choose, to Markdown (a folder or a ZIP with the images), PDF typeset with Typst in Yu Gothic, and the Typst file itself to edit.
- Model manager with resumable, checksum-verified downloads and a 10-second speed benchmark.
- Setup wizard, tray menu, global hotkeys (off until you set them), Japanese and English UI; the first launch follows the Windows display language.
- App icon and logo: a red bird (kiki-tori; とり is a bird) whose outline ripples out in fading waves. In the tray the ripples show only while recording (grey while paused or finishing); the title bar shows the kikitori logo, with the bird as its o, and the PDF footer carries the bird.
- Daily update check that asks before installing (active once updater keys are set up).
- NSIS installer that installs per user without admin rights and bundles the Vulkan loader.
