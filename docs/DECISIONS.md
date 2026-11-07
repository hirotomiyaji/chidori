# Decisions

Choices made where the spec (`AppSpec.md`) is silent, and the few places where implementation
found a reason to deviate.

## Build and toolchain

- **Iced 0.14 instead of Tauri and React** (from 0.6). The WebView2 that drew the Tauri UI was
  most of the app's memory: 99 of 153 MB at idle with the model loaded, and the line cost 18% of
  a core while recording (0.5.1, measured 2026-10-28 with the window in view). A spike of the
  recording window in Iced used 12 MB drawing on the CPU (tiny-skia) and about 119 MB plus 89 MB
  of GPU memory through wgpu (Vulkan; DX12 69 MB).
- **The UI draws on the GPU through DirectX 12** (wgpu, `WGPU_BACKEND=dx12` set at startup
  unless already set). Measured in the finished app (release, turbo-q5, window in view):

  | | Idle RAM | Recording RAM | Recording CPU |
  | --- | --- | --- | --- |
  | 0.5.1, WebView2 | 153 MB | 180 MB (silence) | 18% (silence) |
  | wgpu, Vulkan | 160 MB | 200 MB | 11.7% |
  | wgpu, DX12 | 114 MB | 147 MB | 9.8% |
  | tiny-skia (CPU) | 63 MB | 98 MB | 11.4% |

  Recording rows are with speech being transcribed, except 0.5.1's. Vulkan's driver costs about
  45 MB more than DX12 for the same frames. tiny-skia is lighter still and no slower here; GPU
  drawing was the maintainer's choice. tiny-skia stays compiled in as the fallback where wgpu
  can't start (no GPU driver), and `ICED_BACKEND=tiny-skia` picks it by hand.
- **A Cargo workspace**: `crates/core` (no UI, no windowing), `crates/app` (the Iced UI and the
  shell: tray, hotkeys, popup menus, dialogs, updater) and `xtask` (the installer build). The core
  keeps the module layout it had under Tauri; Tauri's `AppHandle` became `host::AppHandle`, its
  JSON events typed `events::Event`s, and its commands `ops.rs` functions.
- **Exact pins.** Drift-prone crates (`iced`, `tray-icon`, `global-hotkey`, `rfd`, `cpal`,
  `whisper-rs`, `wasapi`, `xcap`, `earshot`, `rubato`, `ulid`, `typst*`, `zip`) are pinned with
  `=`; everything else by `Cargo.lock`, which is committed.
- **whisper.cpp optimization on MSVC.** The `cmake` crate replaces CMake's per-config flags with
  `cc`'s, which carry no optimization level on MSVC, so whisper.cpp built unoptimized: model load
  went from 9 s to 84 s and CPU RTF from 0.39 to 3.5. `.cargo/config.toml` sets
  `CMAKE_{C,CXX}_FLAGS_RELEASE` back to `-O2 -Ob2 -DNDEBUG`, and the same for
  `RELWITHDEBINFO`, the config dev builds use (they were unoptimized too).
- **Static C runtime.** `.cargo/config.toml` adds `-C target-feature=+crt-static` for the MSVC
  target and passes `-MT` to whisper.cpp to match (the flags above replace the `cmake` crate's,
  so it can't). The release exe imports no `vcruntime140.dll` or `msvcp140.dll`; it imports
  only the Universal CRT (`api-ms-win-crt-*`), which is part of Windows 10 and 11.
- **Vulkan loader bundled.** whisper.cpp's Vulkan backend links `vulkan-1.dll`, which only GPU
  drivers install, so the exe would not start on a PC without one. `cargo xtask dist` downloads
  LunarG's `VulkanRT-X64-1.4.363.0-Components.zip` (pinned SHA-256 for the zip and the DLL) into
  the git-ignored `crates/app/resources/vulkan/`, and the installer puts `vulkan-1.dll` next to
  the exe with its licence in `licenses\`. With no driver the loader
  reports no devices and the app runs on CPU.
- **Installer size: about 22 MB**, over the spec's 15 MB target (section 16). Most of the
  difference is the Typst compiler linked in for PDF export: before it, the installer was
  12.6 MB.
- **CI.** Actions are pinned to exact tags and Rust to 1.98.1 (the version the code was built
  with). The short target directory is computed at run time (`$GITHUB_WORKSPACE\..\t`) instead
  of assuming a drive letter. `ci.yml` only checks, and skips pushes that change nothing but
  Markdown or `docs/`; installers come from `release.yml`, which builds one per `v*` tag.
- **rustfmt** uses `max_width = 120` with `use_small_heuristics = "Max"`, the style the code was
  written in; `cargo fmt --check` runs in CI.
- **Licence: MIT**, the spec's default for a public repository.
- **Third-party notices are generated at build time.** MIT, BSD and Apache-2.0 ask for their
  notices to travel with the binary, so `cargo xtask dist` writes
  `licenses\THIRD-PARTY-NOTICES.txt` (about 1 MB): each distinct licence text once, with the
  packages that ship it. It covers the crates linked into the exe (from `cargo metadata`; build
  scripts and proc macros only run while compiling), whisper.cpp (MIT, inside whisper-rs-sys),
  libopus, and the fonts (OFL) and icons (ISC) the UI embeds. Packages that ship no licence
  file are listed with their declared licence and source. Generating it rather than committing
  it means it can't go stale. Typst's
  bundled fonts are not compiled in (`typst-assets` without `fonts`); hayagriva's CSL styles
  (CC BY-SA 3.0) are, and its notice in the file covers them.
- **CPU baseline: AVX2.** Local builds use `GGML_NATIVE` (AVX-512 on a Zen 5 CPU). Release
  builds set `GGML_NATIVE=OFF`, which selects `/arch:AVX2`, because a binary built with
  `/arch:AVX512` crashes on most consumer Intel CPUs. At startup the app checks for AVX2 and FMA
  and shows 「このPCのCPUは文字起こしに対応していません」 instead of crashing.
- **Vulkan builds need a short target directory.** ggml-vulkan builds `vulkan-shaders-gen` as a
  nested CMake project; under a normal checkout its MSBuild FileTracker paths pass 260 characters
  (error FTK1011). Builds with `gpu-vulkan` use `CARGO_TARGET_DIR=%USERPROFILE%\.kt`; CI uses a
  short path too. The Vulkan SDK is needed only to build.
- **The installer is cargo-packager's NSIS**, run by `cargo xtask dist` with a config it writes
  (absolute paths, so it runs from anywhere). Its template comes from Tauri's, so it takes the
  same `/P` (passive), `/R` (relaunch) and `/UPDATE` switches and writes the same uninstall entry
  and install folder for the same product name, identifier and publisher (`yusai`, as the Tauri
  installer recorded it).
- **No Node in the build.** The brand's tokens, logos and fonts are vendored in
  `crates/app/brand/` (fonts as TTF from Google Fonts' repository, which the UI embeds), the
  Lucide icons as SVG in `brand/icons/`, and the translations in `crates/app/i18n/`.

## Transcription

- **Whisper states are freed between recordings.** Each source's `WhisperState` holds about
  280 MB of VRAM (turbo-q5) and 700 MB of commit. The worker drops them once its queue is empty
  after a session is saved, a mic test or a benchmark, and recreates them on the next job.
- **GPU device name** comes from `whisper_rs::vulkan::list_devices()` (device 0, the one
  whisper.cpp uses) instead of parsing whisper.cpp's log output.
- **Flash attention** is on for GPU contexts (whisper.cpp's current default); a failure falls
  back to CPU like any other GPU init failure.
- **Remembered GPU failure** is keyed by model ID and GPU name; whisper.cpp does not expose the
  driver version. Settings → モデル has 「GPUを再試行」 to clear it.
- **GPU crash detection.** Before a GPU init the app writes `gpuInitInProgress` to `state.json`
  and clears it afterwards. If it is still set at launch, the init crashed the process, so the
  app records a GPU failure and uses the CPU.
- **Transcribing after Stop** (FR-25). The queued utterances stay in memory, as every backlog
  does: 16 kHz float audio, about 64 KB a second of speech per source, so an hour of non-stop talk
  holds about 230 MB. Writing them to disk was left out until a recording needs it. Hiding the live
  text a minute behind was considered and dropped: drawing it costs nothing next to Whisper, and
  the lines that do arrive are still worth seeing.
- **Backlog merging.** Inside the worker, "lag ≥ 5 s" is measured as the span between the oldest
  and newest waiting utterance of a session, which needs no clock and only matters when there is
  more than one job to merge. A segment Whisper returns from merged audio belongs to the
  utterance under its midpoint but keeps the span of every piece it covers (more than half of
  the piece or a second of it); clamped to one piece, a mic line of echo hid from the echo guard
  the 相手 lines it repeated.
- **Language 自動.** Utterances are decoded with language `auto` until one of 3 s or longer has
  been decoded; its detected language is then locked. The session keeps `language: "auto"`; the
  detected language is logged as a `language_detected` event.
- **Whisper never translates** (`asr/language.rs`). Told a language the speech is not in,
  Whisper translates into it or loops: with 日本語 set, English speech came out in the grey
  partials as 「このグリースを選択して、押し込みを…」, and a final without a prompt as
  「私は、私は、…」 (finals mostly stayed English only because of their prompt). After each decode
  the worker runs one decoder step on the start-of-transcript token over the encoder output the
  decode left in the state, which is how whisper.cpp detects a language, without a second encoder
  pass (5 ms on CPU; the probabilities match whisper.cpp's own detection). When Whisper is at
  least 0.9 sure a clip is in the other of Japanese and English, the clip is decoded again in that
  one, and the source's next clips start in it while Whisper stays at least 0.5 sure; then they go
  back to the configured language. On real speech, Japanese scored at most 0.74 for English (0.6 s
  fragments and silence, turbo-q5 and base-q5) and English at least 0.9 (turbo-q5: 0.999). Clips
  Whisper is unsure of keep the fixed language, as section 8 asks; 自動 works the same way once it
  has locked.
- **Primer** 「えー、それでは始めます。…」 is used only when decoding Japanese.
- **Partials** run only on GPU, when no final job is queued or running, and when the model's
  benchmark tier is 快適 (or no benchmark has run).
- **Segmenter details.** The utterance keeps up to 320 ms of trailing silence (capped by the
  hangover). Pre-roll never reaches back into the previous utterance, so no audio is transcribed
  twice. The soft maximum cuts at the middle of the quietest 6-frame (96 ms) window in the last
  3 s; the hard maximum is checked before a frame is appended so no utterance exceeds 25 s.
- **Echo guard.** A 自分 line that matches an earlier 相手 line is never written; one already
  written is removed with `segment_removed` when its 相手 match arrives. Besides pairwise Dice
  similarity, the 自分 line is compared with all overlapping 相手 lines joined (the share of its
  bigrams found there), because the mic's VAD often cuts one echo line across several 相手
  lines. With turbo-q5 this took the echo left in the two-channel fixture from 82 characters to 0.
- **Short utterances get no context prompt.** With under 1 s of speech the prompt is the word
  list only: given the previous line, Whisper tended to repeat it (a lone 「四」 came out as
  「よろしくお願いします。」).
- **Mic test** runs the VAD and segmenter over its 10 s recording and transcribes only the
  speech, so a silent test shows 「言葉は聞き取れませんでしたが、続けて大丈夫です」 instead of a
  hallucinated 「ありがとうございました。」.
- **Hallucination list** is matched after NFKC; half-width katakana normalizes to katakana, so
  hiragana phrases do not match their katakana forms.

## Audio capture

- **App picker** lists sessions on every active render device, not only the default one, so an
  app playing to a headset that is not the default still shows up.
- **App capture can start before the app runs.** The capture thread waits for a process with the
  same executable (the reattach loop of FR-14), so the Start hotkey works with a last-used app
  that is closed. The status row says 「Zoom の音声を待っています」.
- **Default-device changes (FR-15)** are debounced for 1.5 s and reopen the stream only if the
  default actually changed. Opening a Bluetooth hands-free mic makes Windows flap the default
  while the headset switches profiles; reopening on every notification thrashed.
- **Device switches** show a notice but no timeline marker; the 「音声ソース再接続」 marker is
  for an app that exited and came back.
- **`read_from_device`** with an exactly sized buffer is used instead of
  `read_from_device_to_deque`, which unwraps the `ReleaseBuffer` result inside `wasapi`.
- **Missing QPC timestamps.** A packet with timestamp 0 or the timestamp-error flag is placed at
  "now minus its duration".
- **Gaps are never materialized.** The clock reports gaps as a frame count and the DSP thread
  feeds silence in half-second pieces, so an app that was closed for minutes costs no memory.
- **Quiet streams.** System loopback delivers no packets in silence; after 500 ms without packets
  the DSP thread pads silence up to 300 ms before now so an open utterance can close.
- **Health checks.** "No packets for 5 s" restarts mic and app streams once; only the mic then
  raises a banner, because process loopback can legitimately go quiet. System loopback is exempt.
- **Files (FR-18) are decoded by Media Foundation**, not a Rust decoder such as symphonia: it reads
  every container and codec Windows plays (the AAC in MP4 and M4A from Zoom, Teams and phones, MOV,
  MKV, WebM, WMA, MP3, FLAC) and a video's audio track, with nothing new to ship; symphonia has no
  Opus, AC-3 or WMA. Its own error codes (`0xC00D…`) read as 「このファイルの形式は読み込めません」,
  a file with no audio track as 「このファイルには音声がありません」.
- **Ogg Opus goes through libopus**, which the app ships for FR-09, because Windows can't read it:
  `.opus` voice messages, and Chidori's own `audio/*.ogg`, so a session can be transcribed again
  with another model. Only mono and stereo (mapping family 0).
- **A file session is a session with one `file` source** (相手), not a new kind, so History,
  playback, projects, exports and recovery work unchanged. It is named after the file, and
  `startedAt` is when it was transcribed, so History lists it on that day. Every time it shows is
  the place in the file (`00:12:34`), never a clock time: when a file was recorded is unknown (its
  modified time is often when it was downloaded or copied). 続きを録音 onto one is allowed; the
  added part's times are places in the session too.
- **It runs as finishing from the start** (the `finishing` state): pause, cut, star, screenshots
  and the lag notices mean nothing for a file, and finishing already shows progress, saves at the
  end and offers 処理を中止. The bar is by file time: what was read, minus the speech still queued.
- **Reading waits while 2 minutes of speech are queued.** That keeps memory bounded (queued speech
  is 16 kHz float, about 230 MB an hour) while keeping the queue far enough behind that jobs merge
  into 25 s ones. Measured on the meeting fixture (turbo-q5, RTX 5070 Ti, 2026-10-23): 3 minutes
  of sound in 1.4–3 s from MP4, M4A, WebM, FLAC and Opus, CER 20–25% against one-pass 22.9%.
- **Unprocessed audio**: a cancelled file, or one that broke off, marks what was queued and what
  was never read as unprocessed. A file read to its end never does, though containers often state
  a length a little different from the sound's (an MP4's video track may run longer).

## Sessions and exports

- **`session.json`** also stores the SHA-256 of files Chidori wrote (`written`), which is how
  an edited `transcript.md` is detected and never overwritten.
- **Rename** changes the title only; the folder keeps its original name.
- **Projects live beside the sessions** (FR-64): `projects.json` in the output root holds each
  project's ID, name and colour, and a session logs only the ID (`project_set`), so the list
  moves with the folders and a rename shows everywhere at once. Deleting a project leaves its
  sessions' IDs in place, naming nothing, so they read as in no project and no session folder is
  rewritten. The Markdown front matter looks the name up beside the folder.
- **A cut is a marker** (`cut`, FR-06), not a new session: the part shares the session's
  sound, line and folder, and a cut needs no new event in the log. It works while paused, so
  a break can end a part. Markdown heads the part with `## HH:MM:SS`, the PDF with the time
  over a hairline as a heading (so the PDF outline lists the parts), and plain text with
  「— HH:MM:SS 区切り —」. Parts have no names yet; the marker's `detail` would hold one.
- **"The line being said" is an utterance, not a time** (FR-07). A line's text arrives seconds
  after it is spoken, so the star cannot just mark the newest line: while someone speaks that
  is the line before. Each DSP thread notes the utterance it has open and the last one it
  closed (`SessionCtx::speaking`); the star takes every utterance open at that moment, else the
  one that ended last, and the store keeps the mark until that utterance's text arrives. Whisper
  may split one utterance into several lines; the mark goes to the one spoken at the moment of
  the press. An utterance that gives no line (too short, a filtered hallucination) drops its
  mark. Marking from the transcript works on any line, live or after, through `mark_segment`.
- **Important lines in exports** start with `★ `, a character an agent reads as plainly as a
  person, rather than bold or highlight markup, which differs between Markdown flavours and
  would be lost in plain text. An important line is its own paragraph, so the star never
  claims a neighbour's words.
- **Switching the source is a new capture plan, not a new session** (FR-17). Capture stops,
  the DSP threads flush their last utterances, and the new plan starts at the same session time,
  as a resume does; the session adds the new source to its list (so two apps may both be 相手)
  and logs a `source_changed` marker naming it in Japanese, like every export string. 相手 and
  自分 labels and the echo guard now ask whether both sides were ever recorded
  (`Session::two_sides`), not how many sources there are, so Zoom → Teams alone stays unlabelled.
  If the new source fails, the old plan restarts and the picker goes back to it. While paused
  the new plan waits for resume. 「システム全体に切り替える」 after a silent app is the same switch.
- **Continuing a session appends, it does not merge** (FR-08). The new recording starts at the
  next 100 ms step after the session's end, so the transcript, the screenshots and `levels.bin`
  carry on in one session time, and each continuation records its wall-clock start
  (`Session::continued`) so the clock times after it stay true even days later; the timeline
  marks the seam with a `continued` marker (with the date when it differs). Session zero then
  lies the session's length before now, which may be before the PC started, so the recorder's
  and the stream clock's `t0` are signed. IDs carry on after the highest each kind used in the
  log, removed lines included. `session_continued` is logged only once capture runs, so a
  source that fails leaves the session as it was; a session that ended in a crash is recovered
  first. The title, project and model of record stay the session's; the source is whatever the
  picker holds now, and new sources join its list.
- **The sound is one Opus file per recording** (FR-09). The DSP threads already make 16 kHz
  mono audio placed in session time, so they hand it to one writer thread, which adds the
  sources together by position (a source that sent nothing for 1.5 s stops holding the mix
  back), fills a pause with silence, and encodes 20 ms frames. Opus (libopus 1.6, built from
  `opusic-c`'s bundled source) at 24 kb/s keeps speech clear in about 11 MB an hour, against
  115 MB for WAV. The app plays it through `cpal`, decoding with the core's own Ogg Opus reader
  (`screens/main/playback.rs`); with no seek in the reader, playing from a point decodes up to
  it, which takes a moment far into an hour. A page, and a flush, about once a second: a crashed
  recording's file plays up to its last second. One
  mixed file rather than one per source, because playback is for listening back, where both
  sides belong together. A continuation writes the next file, as an Ogg stream cannot simply be
  appended to; playing runs from one into the next. Encoding takes well under 1% of a core. It
  is on by default (the point of keeping it is having it when wanted) and can be turned off in
  設定 › 音声.
- **Unprocessed audio** after 処理を中止 is recorded as an `unprocessed` marker and
  `unprocessedMs`, rendered as 「（以降、未処理の音声 N 秒）」.
- **Exports are Japanese** in any UI locale: the session stores 自分 / 相手 and the spec defines
  the export strings in Japanese.
- **Joining text.** Besides the spec's Latin-letter/digit rule, a space is inserted after Latin
  sentence punctuation (`. , ! ? ; :`) before a Latin letter, so English sessions read naturally.
  Japanese joins are unaffected.
- **Markdown escaping.** `\ * _ ` [ ] < >` are escaped in transcript text, and a paragraph that
  would start a heading or list is escaped. Front-matter values are YAML-quoted when needed.
- **PDF through Typst** (section 11). The `typst` crate compiles the PDF in process from
  `export/transcript.typ`, so no hidden print window is needed. The session becomes a Typst
  document: the preamble, then one call per block (`#entry`, `#shot`, `#note`) with every piece of
  session text in a string literal, so none of it can be read as Typst syntax. Calls rather than
  a JSON data file keep the exported `.typ` readable. The preamble alone decides the look: A4,
  18 mm margins, Yu Gothic (Meiryo and MS Gothic if missing), the brand's ink with 相手 in 朱,
  子持ち罫 under the title (the kamon's double line marks the top of a printed page), and a footer
  with the app's name, small. Zen Kaku Gothic New is not used here: it is bundled only as web
  fonts for the window. Fonts are read from `%WINDIR%\Fonts` for each export
  rather than kept in memory (about 15 ms), and Typst's memo cache is cleared after each one. A
  one-minute session with a screenshot renders in about 0.2 s (debug build) to 330 KB. Typst is
  Apache-2.0: its LICENSE and NOTICE are installed in `licenses/`. The print HTML stays only as
  the fallback, opened in the browser when Typst fails.
  `cargo run --example export_pdf -- <folder> <out.pdf>` renders a session without the app.
- **Typst file.** 書き出し → Typst saves that same document, so `typst compile` gives the app's
  PDF (checked with the standalone Typst 0.15 compiler on a real session saved under a name with
  a space).
- **Markdown is one option with a ZIP switch.** Off, the user picks a directory and gets a new
  folder named after the session (`-2` if taken) holding `transcript.md` and `images/`, the
  session folder's own layout, so links stay `images/…`. On, the same two go into one ZIP (no
  PDF), written through a `.zip.part` file so a failure leaves nothing half-written. The switch
  is remembered while the app runs; the compact window's menu has both as separate entries.
- **Every export asks where to save it** and none is written into the session folder: a save
  dialog over the window, starting in Downloads, suggests the session folder's name. Typst
  copies its screenshots into `<name>_images/` beside the file (named after it, so an `images/`
  already there is untouched) and links them there. The PDF opens when saved; the others only
  say where they went. If Typst fails, the print HTML goes where the PDF was to go, its
  screenshots as `file:///` links into the session folder. `transcript.md` is still written into
  the session folder on Stop: it is the recording's own copy, not an export.

## Name

- **Chidori, formerly Kikitori.** The kamon brand names each app after the motif in its logo, and
  this app's logo is the plover of 波に千鳥, so it is Chidori — 文字起こし / Transcription. The name
  alone sits in the window title, title bar, tray and notifications; the name with its descriptor
  in the README titles and the store-like places.
- **Everything was renamed**, the identifier included (`dev.yusai.chidori`, crates `chidori` and `chidori-core`,
  the GitHub repository `chidori`, whose old address GitHub redirects, so Kikitori's
  updater still finds new releases). The repository has since moved to the `yusai-apps`
  organisation (`yusai-apps/chidori`); GitHub redirects `yusaitajiri/chidori` too, so copies whose
  update endpoint still names the old owner keep updating. The app's folders are named after the identifier and the
  install after the product name, so two things carry existing installs over: the installer
  silently uninstalls Kikitori before installing Chidori (`REMOVE_KIKITORI` in `xtask`; a silent
  uninstall keeps the app data), and on its first launch Chidori moves `%APPDATA%` and
  `%LOCALAPPDATA%\dev.yusai.kikitori` into its own folders entry by entry (never copying the
  models) and renames `Documents\Kikitori` to `Documents\Chidori` while the save folder setting
  still points at the old default (`migrate.rs`). A folder it can't move stays where it is and
  keeps working: the old save folder stays the setting.
- **The updater key keeps its file name** (`~/.tauri/kikitori.key`): the key itself is what
  existing installs trust, and `scripts/enable-updater.mjs` still defaults to that path.

## UI

- **Look: the kamon brand.** The kit's `tokens.json` (from `@yusai/kamon-react`, the
  `kamon-brand` repository pinned to a `react-v…` tag, copied by `pnpm brand`) becomes Rust
  constants in `build.rs`, with Chidori's 朱 as `accent`; `widgets.rs` holds the kit's components
  (the primary with its inner line, the frame, the rule, switches, fields, dialogs) built only
  on those tokens, and `theme::p()` gives the light or dark palette, following Windows. A warm
  grey ground (`surface`), raised cards with hairlines, ink, and one colour, Chidori's 朱. 朱 marks what you can act on and what is selected, as
  the brand asks, and in this app also **the voice you hear**: 相手's line and name, the live mark
  and the tint under the line. That is the one place the app goes past the brand's rule, because
  the line and its speaker are what the app is about, and the plover in the logo is the same 朱.
  自分 is ink. Errors and warnings are the brand's brick and ochre on their tints with an icon and
  words (朱 sits between them, so colour alone never carries an error). 「遅れ n秒」 30 s or more
  behind (FR-25) is the brand's warning ochre. It (and 一時停止中) is written on the line's left
  end, on the ground, since the row under the line has no room left beside the timer. No glows,
  gradients, blur or shadows except the brand's one shadow on what floats (menus, dialogs, toasts).
- **The kamon's double line** appears in its three places, lighter than the brand's default: 子持ち罫
  only in the setup wizard, under its title and step bar, which together are its app bar (a rule
  under the title bar and another line under the steps read as one line too many); the app's other
  views have no rule, and the title bar sits straight on the view. The inner line is on each
  view's one primary button (開始 before a recording, 停止 while recording, 保存 in a dialog), and
  the frame on the container being worked in (a dialog, and a transcript line while it is edited).
  The recording view keeps 停止 as its primary while a line is edited, since stopping must stay
  one tap away.
- **開始 is round**, a primary button with the record dot and no word (its name is in the tooltip
  and for screen readers), 64 px in the middle of the expanded window and 36 px on the compact
  line. The brand keeps `radius-full` for people, so this is a departure: the round record button
  is the one shape everybody reads as "start recording", and it is the only round control.
- **The window opens expanded** (420 × 640) until it is shrunk; the compact window is remembered
  once chosen.
- **Text on its way.** While recording, speech 1.5 s or more behind (the backend's lag, FR-25)
  shows 「文字起こし中…」 with a spinner under the last line, or in place of 「話し声を待っています…」
  before the first one, unless a grey partial already shows the words coming. A fast GPU writes a
  line within that time, so the note is for slow PCs, where text can take many seconds. 仕上げ中
  has a spinner too, since one long stretch can keep its count still.
- **Type.** Zen Kaku Gothic New (400 and 500) and IBM Plex Mono for numbers that line up (the
  timer, times), embedded in the exe (TTF from Google Fonts' repository, OFL) so the window never
  fetches fonts. The transcript is the brand's body,
  16 px on 27 px. The windows are small (380 and 420 px wide), so the UI's own text stays at 12 to
  14 px with `body-small` as the base, rather than the brand's 16 px everywhere.
- **The line is real sound** (`screens/main/deck.rs`, `screens/main/live_line.rs`,
  `model/line.rs`). While
  recording, the line across the window is drawn from the levels the backend sends ten times a
  second: 相手 rises above it in 朱 and 自分 dips below it in ink (a single source rises above,
  whichever it is), lightly blurred so a lone step is not a spike. A pen tip moves right from
  where the line starts and, near the end, the line scrolls under it. Screenshots are dots on
  the baseline and a pause leaves a dotted gap. While finishing, the steps not yet written are
  grey and fill in as the queue empties, with the pen waiting where the written part ends. The
  steps belong to the app (`LineHistory`, fed in `app.rs`), so the line survives a visit to
  History or Settings. The line is the edge of the tint, so it runs from edge to edge: the pen
  starts in line with the buttons under it (14 px, compact 12), the tint rises to the top of the
  line, and where nothing is written, before the pen started and ahead of it, the line is grey
  and as heavy as the written one, coming back down to the baseline just after the pen. Each
  stretch rises from the baseline where it starts and drops back to it before a pause, so the
  line never ends in mid-air. These are FR-03's per-source meters: a decorative wave would look
  the same in every meeting and could not show a silent mic.
- **A session's picture is its sound.** A shape built from when each side spoke is flat in
  continuous speech, where every slice has speech, so the recorder keeps the 10 Hz levels in
  `levels.bin` (2 bytes per 100 ms, paused time silent), and History and the finished session
  draw 128 slices of mean loudness, raised together so the loudest stands full height (at most
  2×). A session without `levels.bin` falls back to when each side spoke (`shapeOf`; Rust
  computes the same as `SessionSummary.activity` for History).
- **Every session keeps its shape.** When a session is saved the pen lifts and the line zooms out
  to the whole session, with dots for screenshots. Pointing at it shows the clock time; clicking
  scrolls the transcript there and marks the row (`seek` in the transcript store). The same shape
  is the session's picture in 履歴.
- **開始 is the primary button.** Before a recording the expanded window has no transcript to
  show, so the deck takes the free space under the source picker, and 開始, the brand's primary
  button with its inner line, sits in its middle: centred in the space above the model's line at
  the bottom and lifted a little to the optical centre (45% of that height), because at the exact
  middle it looks low. There is no line yet; a hotkey, when one is set, sits under it, and the
  model at the bottom edge. Starting (button or hotkey) fades 開始 while the line comes down to
  the bottom and the pen tip grows in at its left end; the deck eases from its full height to its
  strip as the transcript opens above it. The deck stays mounted from the start screen to the
  finished session, so no cross-screen morph is needed. 「新しい録音」 flattens the session's line,
  the pen fades as it rises, and 開始 comes back. The compact window has no free space: 開始 sits
  on the left end of an empty line, where the pen will start. The brand keeps round shapes for
  avatars, so 開始 is not a circle; the pen tip and screenshot dots
  are marks on a chart, not controls.
- **The logo is the brand's kamon.** The plover in the 木瓜 frame, in 朱, from the kit: the title
  bar shows its small drawing beside the name (`<Logo app="chidori">`), the app icon is generated
  from the kit's `icons/chidori/icon-1024.png` with its `icon.ico` copied over (it draws the small
  sizes properly), and the tray shows the kit's small icon (`icons/tray/`), in ink while idle,
  paused or finishing and in 朱 while recording. The logo is never redrawn or recoloured here.
- **What to listen to is a source and a conversation switch.** The source opens one list:
  システム全体, マイクだけ, then the apps (those playing first, その他 after, with a refresh
  button, FR-11). Under it, 「会話として録音」 is FR-12's mic switch, worded as what it is for,
  and its own list picks the mic (the same setting as 設定 › 音声). The names 相手 and 自分 appear
  only in the transcript: on the switch they would mislead for recordings that are not a
  conversation (a lecture, a video). With マイクだけ the second row only picks the mic. The
  compact window shows the choice as one line (`Zoom · 会話`) that opens the picker over the
  whole window, and loads the app list when it appears, so the line carries the app's icon.
- **Apps show their sound.** In the source picker each app with an audio session shows its sound
  beside its name while it plays (`AppWave`): the last two seconds as a small 朱 line on a grey
  baseline, in the field and in the list. It is real sound, not a decorative wave: the peak
  meters Windows keeps for every audio session (the volume mixer's bars), read by
  `audio::win::meters` only for the apps on screen, twenty times a second, with nothing captured
  and nothing sent in silence. A read takes about 30 µs and listing the sessions about 2 ms, so
  one app costs ~0.15% of a core. The wave steps ten times a second instead of gliding every frame
  like the recording's line: redrawn at the screen's rate it cost about 40% of a core on a 165 Hz
  screen (measured in the web UI before the rewrite), stepping about 4%, and nothing once it
  is quiet.
- **Navigation is words in the title bar**: 録音 · 履歴 · 設定 in the expanded window, the current
  one in the accent's tint (the brand's active nav), 録音 with a small 朱 mark while recording; the compact window keeps its icons. While
  recording the title bar shows the state and the source in place of the name, and away from the
  main view that leads back to the recording. No bottom tabs: a phone pattern that would add a
  second way to navigate, the largest coloured area in the app, and a second layer of controls
  under the deck while recording.
- **One title bar** over every view (`screens/titlebar.rs`, drawn by `app.rs` above the view it
  shows); the back arrow follows the view. The compact toggle is on every page but the wizard: only 録音 has a
  compact form, so shrinking from 履歴 or 設定 goes to 録音.
- **Big headers.** History, Settings, the model list, the wizard and a finished session open
  with a big title on the ground above a hairline. A finished session's header scrolls with its
  transcript: the title (click to rename), when it started, its length, lines and screenshots in
  big light numerals, and 詳細, closed at first, for the model and the folder it was saved to. A
  finished transcript gets 476 of 640 px. The header doesn't say where the session was saved
  (「保存しました · Chidori › …」 read like a garbled 「Chidori › … に保存しました」); saving
  shows a toast instead.
- **Settings is a list** of its sections, each showing what it is set to now; a section opens with
  a back arrow. A pill tab per section would overflow the 420 px window with no sign that the
  tabs scroll.
- **Projects are coloured squares** (FR-64), the one place colours beyond ink and 朱 appear, so
  they stay small: a 10 px rounded square before a title, never a fill, a chip or a tinted row.
  Seven colours from the brand's palette at the same depth (ink-subtle, 藍, 鉄, 苔, ochre, 葡萄 and
  blush), light and dark, none of them 朱 (朱 is the voice you hear); a new project gets one no
  other project uses. The brand keeps other apps' colours out of an app; these squares are labels
  the user picks, not the app's colour, so they are the one exception. A session joins a project from the swatch beside the recording's title, the line
  under a finished session's title, History's row menu (a submenu) or 選択 (a button beside
  削除), each a native menu ending in 「新しいプロジェクト…」. History filters by project with a
  menu button beside the search, shown only once a project exists. **A project's home is
  履歴, not 設定**: filtered to one, the page is that project's, its square and name in place of
  「履歴」 and its count beside them, with a ⋯ to change its name and colour (the new-project
  dialog, filled in) or delete it. A project is a way into the sessions, not a preference, so it
  is changed where its sessions are shown; 設定 › プロジェクト is gone. The filter menu also ends
  in 「新しいプロジェクト…」, which opens the new, empty project's page. A new recording starts in
  no project: one tap on the swatch puts it in one, and a default that carried over would file
  recordings by surprise.
- **History is grouped by day** (今日 and 昨日 with the date, then dates) and searchable by title
  and opening words (`SessionSummary.preview`), each row with its title, time, line, opening
  words and length. A recording shorter than 10 s or with no transcript (most likely a test or
  started by accident) is greyed out and has no line; it still opens, and its menu still deletes
  it. 選択 picks several recordings to move to the Recycle Bin at once (`delete_sessions`), with
  quick picks for all and for the greyed short and empty ones.
- **Playing back** is a ▶ in each line's hover actions (any line plays from where it starts) and
  a play button first in a finished session's row; while it plays the button is inverted like
  the paused pause button, the line being played has the soft background a jump uses, and a
  2 px ink playhead moves along the session's line, stepping four times a second with a 250 ms
  glide instead of redrawing every frame (see the cost of per-frame redraws above). The
  transcript keeps up with the sound unless the reader has scrolled away from the playing line.
  Not while recording: the file is still being written.
- **続きを録音 is in the ⋯ menus** (a finished session's and History's rows), not a button of its
  own: the finished session's row already holds four buttons in the compact window, and
  continuing is rarer than starting anew. It records with the source the picker holds and opens
  the recording view; while something records, it is greyed out.
- **A session has one ⋯ menu** (`screens/session_menu.rs`, docs/OBJECTS.md), wherever it is shown, so
  it offers the same actions open or as a row: 復元 (a crashed session), 続きを録音, フォルダを開く
  and 削除. A History row, which has no deck or header, adds コピー ▸, 書き出し…, 名前を変更 and
  プロジェクト ▸. Rename and delete confirm in dialogs shared by both (`screens/dialogs.rs`). Deleting
  the session on screen takes it off: the main view goes back to 開始, a past session to 履歴.
- **A finished session's buttons**: 再生, コピー ▾, 書き出し, which opens a dialog to pick
  Markdown, PDF or Typst (the compact window, too small for it, shows a menu), the ⋯ menu, and
  「新しい録音」. コピー copies plain text and its chevron picks 「Agent用にコピー」 or
  「Markdownでコピー」, joined into one control the way the camera and its menu are: the three
  are one action in three formats, so they sit together rather than split between a button and
  ⋯.
- **Less text.** Choices have no descriptions under them (the source field is one line,
  「会話として録音」 shows only the mic's name, the export dialog lists formats only, settings
  have no help lines, the wizard's paragraphs are one short line or none), and labels are short
  (「ソース」). Pages and wizard steps change without a slide.
- **Always on top is off by default**, toggled by a pin in the title bar on every page.
- **Screenshots take the recorded app by default**: its window while recording an app, the
  cursor's screen while recording system audio or the mic alone (`appWindow`).
- **The camera has a menu** (a chevron joined to it, FR-34), a native popup like ⋯ so it fits
  the compact window. The three targets are the setting itself, so a choice there is
  remembered, as in 設定 › スクショ; a window from its list (up to 20, front first, Chidori's own
  left out, from the same `xcap` listing) holds for the rest of the recording only, since a
  window seldom outlives one. A picked window that has closed or been minimized falls back to the
  cursor's screen, with the usual 「（カーソルの画面）」 toast. The picked window travels in
  `recording://state` (`shotWindow`), so the menu's check marks follow the backend.
- **Speaker names** are coloured text: 相手 朱, 自分 ink. A darker 朱 for 自分 would be hard to
  tell from 相手 and nearly identical with red-green colour blindness. Provisional text is the
  brand's subtle ink.
- **Hairlines part a screen.** The only waves are the sound. Corners are the brand's: 8 px for
  buttons and fields, 12 px for cards, menus and dialogs, 6 px for tags and switches, and no pill
  anywhere. Dialogs are centred sheets in the kamon's frame with full-width buttons. Transcript
  markers (pause, reconnect) keep a straight line. Weights are 400 and 500 only, the two the
  brand's font is bundled in.
- **Japanese line breaks** follow Unicode's line breaking (cosmic-text), so a line can break
  inside a word such as ヘッドホン or ください. The web UI broke between phrases
  (`word-break: auto-phrase`); doing that here needs a phrase splitter of our own.
- **Only what moves is redrawn.** The app ticks only while something moves: the live line about
  60 times a second while recording (30 in the compact window, on screen through whole
  meetings), the waves and spinners 20 times, toasts 10; otherwise it draws only when something
  changes. Minimized or in the tray nothing ticks at all, and the core stops reading the apps'
  meters (`host::set_ui_hidden`), while its events still arrive, so the transcript keeps up.
- **The transcript builds only the rows in view** (`screens/main/transcript.rs`): rows not yet
  drawn get an estimated height and report their real one once drawn, inside one `lazy` keyed on
  the transcript's version, so a line frame never rebuilds the transcript. Iced lays out what it
  is given every frame: in the spike, 2000 plain rows cost 13% of a core and 71 MB.
- **The transcript runs on under the deck to its line.** The line lies 30 px into the deck; if
  the transcript stopped at the deck's top, text would vanish at an invisible edge above the
  wave. Once there is a transcript the expanded deck reaches up under it by that much and is
  see-through above the line: the text goes right up to the wave and only the tint under the
  line covers it. The deck lets the pointer through to
  the text, except on its buttons and, after a recording, a band around the line where pointing
  shows the time. Scrolled to the end, the last line still rests clear of the deck.
- **Transcript.** Lines are grouped into turns: with both 相手 and 自分 recorded the speaker,
  then the time, head each turn on its own line, and the text runs the full width (a new turn
  starts at a speaker change, after a picture or marker, and every minute). There is no time
  column: at 52 px it would push the text to x = 80 and wrap lines after about 23 characters.
  With one source there are no speaker names.
- **Screenshot placement is strictly chronological** (section 9). Placing a screenshot after
  every sentence containing its time put it after sentences that started later: Whisper often
  cuts one utterance into back-to-back segments, and a segment starting exactly at the
  containing one's end counted as inside.
- **最新へ jumps from far away.** More than two screens above the newest line it scrolls there
  at once instead of gliding: in a long session left in the background the glide took seconds,
  and rows measured on the way kept moving the end.
- **The recording keeps a header**: the title (click to rename, as in a finished session) and
  the source as one line (`Zoom · 会話`), above the transcript and outside its scroll, so both stay
  in view in a long session. It is small (one line, 15 px title) because the transcript is what
  the window is for. The source is a dropdown under that line, right-aligned and at least 280 px
  wide, with the same rows as the start screen's list (the apps with their sound) and
  「会話として録音」 as a switch under them; each change switches at once (one tap, no 適用
  button), and a toast confirms it. Not the start screen's picker over the whole window: a
  dropdown keeps the transcript in view. The compact window has no room for a header or a
  dropdown, so its title bar's 録音中 tag opens a native menu with the same choices, which may
  reach past the window; away from the main view the tag still leads back to the recording. The
  switching itself (`useLiveSource`) runs for the whole recording, whichever control changed the
  choice, and first takes the choice from the settings without switching, since a recording
  started by the hotkey uses the last-used source the picker never showed.
- **The star** sits between the camera and the cut while recording, and on every line's hover
  actions (live too, unlike editing). An important line has an ink stroke in the left margin
  and medium-weight text; 朱 stays the voice you hear. The compact window's two lines show a star.
- **More hotkeys.** Important mark and cut have hotkeys too (off until set), since a meeting
  app is usually in front. Pressed from another app, they confirm with a toast; all four share
  one registration loop, and one set to the same keys as another is reported, not registered.
- **The cut button** (scissors) sits after the camera while recording. A cut shows in the
  transcript as a marker with a darker rule and more space above it, and a toast confirms it,
  since the compact window's two lines leave markers out.
- **The timer leaves out paused time** (`StatePayload.elapsedMs`); session times still count
  it, so the clock times in the transcript stay true.
- **Animations** are the deck's own: the start, settling into the whole session, and 新しい録音
  rising back. Rows and buttons don't fade in, and Windows' reduce-motion setting isn't read yet.
- **The session viewer has its own transcript** (`screens/viewer.rs`), so a
  recording that runs while a past session is open keeps receiving its lines.
- **The … menu is a native popup menu** (`native/menu.rs`, through `muda`), because the compact
  window (380×170) is too small to hold a dropdown and a menu drawn by the app cannot reach past
  its window. A pick comes back as an id, routed by its prefix to the screen that opened it.
- **The source lists are drawn**, not native (`screens/main/popover.rs`), because they show each
  app's icon and live wave. The compact picker opens over the whole window under the title bar.
- **No screen reader or keyboard navigation yet.** Iced 0.14 exposes nothing to screen readers
  and moves keyboard focus only between text boxes, so the web UI's `aria-label`s and tab order
  have no counterpart. Icon buttons keep their names as tooltips; Escape, Enter and the hotkeys
  work. Revisit when Iced ships accessibility.
- **Text boxes save on Enter** (and the check button): Iced has no event for a box losing focus.
  設定's word list and title format also save at the next click anywhere.
- **One primary per view, strictly**: 使う in the model list and テストを開始 in the wizard's mic
  step are secondary buttons, since a list can hold several models and 次へ is the step's primary.
- **Closing the window** hides it to the tray only while recording or finishing; when idle it
  quits (FR-93 only covers the recording case).
- **Window position** is remembered with `tauri-plugin-window-state`; the size follows the
  layout. Settings, History and the wizard use the expanded size temporarily.
- **Hotkeys are off until set.** Both default to empty: obvious choices such as `Ctrl+Alt+R` and
  `Ctrl+Alt+S` are often held by another app already, and a warning at every launch would be
  worse than no shortcut. 設定 › ショートカット sets each one or turns it off again; an empty one
  is never registered. A shortcut that fails to register is reported only there (FR-92).
- **Shutter sound** (設定 › スクショ, off by default) is two short bursts of decaying noise that
  `screenshot::shutter_wav` builds at runtime, so the installer ships no audio file. It is played
  from Rust with `PlaySoundW` once the capture succeeds, so it also sounds while the window is in
  the tray.
- **Hotkey recorder** requires Ctrl or Alt (function keys may stand alone) so a shortcut can never
  swallow normal typing.
- **i18n placeholders** use single braces (`{n}`) to match the spec's string table.
- **Notices** carry an i18n key plus `params` and a `toast` flag so Rust never formats UI text.

## Languages

- **The first launch follows the Windows display language** (`GetUserDefaultUILanguage`):
  Japanese on a Japanese Windows, English on any other, so the setup wizard is already in the
  user's language. Once saved, the choice in 設定 › 一般 wins.
- **The UI names the sources.** The state the backend sends says which sources are recorded and
  the app's name (`StatePayload.sources`), and the title bar words them in the UI language
  (`Zoom + マイク`, `Zoom + Mic`); hovering shows all of it when the bar is too narrow. 「マイク」
  is the short form, so the English word is "Mic".
- **A new session's default title** names the source in the UI's words at the start: the app's
  name, システム全体 / All system audio, or マイク / Mic. The title is the user's to rename;
  exports stay Japanese (see Sessions and exports).

## Updates

- **The app checks for itself** (`crates/core/src/update.rs`, `crates/app/src/updater.rs`): it
  reads `latest.json` from the newest GitHub release, downloads the installer it names and checks
  its minisign signature against `crates/core/src/update.pub`. The manifest format, the key and
  the installer's switches are the Tauri updater's, so copies installed before the rewrite take
  the first Iced release as an ordinary update, and later ones the same way.
- **Launch check** runs 10 s after launch and at most once a day; the time is recorded when the
  check runs, whatever its result. A found update is a banner 「新しいバージョン x.y.z があります」
  with 「更新する」.
- **Install** is refused while recording or finishing, and the recorder lock is held from the
  last check until the installer starts, so a recording can't begin in between. The passive NSIS
  installer closes the app and starts it again (`/R`).
- **Release signatures are checked before publishing**: `xtask dist` verifies the new installer's
  `.sig` against the app's public key, so a release signed with another key fails in CI instead
  of on every user's PC.

## Security

- **No web content.** The UI is native; nothing renders HTML or runs script, so the Tauri build's
  capability list and CSP have no counterpart.
- **Opening a folder** only opens paths inside the output root, the models folder or the log
  folder (`ops::allowed_path`).
- **TLS** uses rustls with the ring provider (installed at startup).

## Test assets

- **Benchmark clip** `crates/app/resources/bench_ja.wav`: 10 s (4.0–14.0 s) of chapter 4 of the
  LibriVox recording of 新美南吉『ごんぎつね』, released under **CC0 1.0**
  (archive.org item `gongitsune_um_librivox`). 16 kHz mono 16-bit, 320 KB.
- **Integration fixtures** are built by `tests/fixtures/build_fixtures.py` from the same CC0
  recording and the public-domain Aozora Bunko text (card 628) instead of being committed, which
  keeps several megabytes of audio out of the repository. See `tests/fixtures/README.md`.

## Measured results

On the reference machine (RTX 5070 Ti, Vulkan), 2026-10-02. Rechecked on 2026-10-05 before the
first release: the same turbo-q5 CERs, meeting latency p50 509 ms and p95 660 ms.

| Fixture, model | One-pass CER | Streaming CER | Latency (end of utterance → text) |
| --- | --- | --- | --- |
| meeting, turbo-q5, real time | 22.9% | 23.8% | p50 504 ms, p95 639 ms (37 utterances) |
| two-channel, turbo-q5 | — | 相手 23.0%, 自分 23.6% | no echo lines left |
| meeting, base-q5, GPU | 28.9% | 33.1% | — |
| meeting, base-q5, CPU (AVX2, the nightly setup) | 29.1% | 33.1% | p50 810 ms, p95 862 ms |
| two-channel, base-q5, CPU | — | 相手 33.4%, 自分 31.3% | no echo lines left |

turbo-q5 meets section 16 and 18. base-q5 loses about 4 points when cut into utterances, so the
nightly CPU run with base-q5 allows 6 points (`CHIDORI_CER_MARGIN`) and serves as a regression
guard; the three-point rule is checked with turbo-q5 here. Its two-channel 相手 CER sits 1.6
points under the test's 35% limit, so small decoding differences on the runner could trip it.
`CHIDORI_REQUIRE_FIXTURES=1` makes a missing model or fixture fail the run instead of skipping
it.
