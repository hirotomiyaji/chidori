//! Builds the installer (section 17).
//!
//!   cargo xtask dist        the release installer, as CI builds it (Windows: Vulkan build, NSIS;
//!                           macOS: Metal build, Chidori.app in a .dmg)
//!   cargo xtask notices     only writes the third-party licence list
//!
//! On Windows `dist` builds the app with the Vulkan backend, bundles Khronos' Vulkan loader next to
//! the exe (so the app starts on PCs without a GPU driver and falls back to the CPU), writes the
//! licence list the installer ships, and runs cargo-packager. Set `CARGO_PACKAGER_SIGN_PRIVATE_KEY`
//! (and `..._PASSWORD`) to sign the installer for the updater.
//!
//! On macOS it builds the Metal app, has cargo-packager make the `.app`, signs it ad hoc and packs
//! it into a `.dmg` with `hdiutil`. The Mac updater only points at the release page, so nothing
//! there is signed with the update key.

mod notices;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use sha2::{Digest, Sha256};

const VULKAN_RT_URL: &str =
    "https://sdk.lunarg.com/sdk/download/1.4.363.0/windows/VulkanRT-X64-1.4.363.0-Components.zip";
const VULKAN_RT_ZIP_SHA256: &str = "a25a927aa8b9f0371048f1861cf88ac3b9bc9b1fb332c42d897c8ab32695769a";
const VULKAN_RT_DLL_SHA256: &str = "e1fcfc9489beefa6a6d9d11d6c7517f6a2b7f16e0d3fb8f4103bee0210f314a3";
const VULKAN_RT_FOLDER: &str = "VulkanRT-X64-1.4.363.0-Components";

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dist") => dist(),
        Some("notices") => notices::write(&root()).map(|p| println!("Wrote {}", p.display())),
        _ => Err("usage: cargo xtask dist | notices".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask sits in the workspace").to_path_buf()
}

fn app_dir() -> PathBuf {
    root().join("crates").join("app")
}

fn sha256(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    Ok(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed ({status})")) }
}

/// Fetches the Vulkan loader and its licence into `crates/app/resources/vulkan/`, checked
/// against pinned hashes.
fn ensure_vulkan_runtime() -> Result<()> {
    let dir = app_dir().join("resources").join("vulkan");
    let dll = dir.join("vulkan-1.dll");
    if dll.exists() && sha256(&dll)? == VULKAN_RT_DLL_SHA256 {
        return Ok(());
    }
    println!("Fetching the Vulkan runtime from {VULKAN_RT_URL}");
    let tmp = std::env::temp_dir().join("chidori-vulkan-rt");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let zip = tmp.join("rt.zip");
    // Windows 10 and later ship curl.exe and bsdtar (tar.exe, which reads zip files). Call them
    // by path: Git Bash puts its own GNU tar, which can't, first on PATH.
    let system32 = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32");
    run(Command::new(system32.join("curl.exe")).args(["-sSfL", "-o"]).arg(&zip).arg(VULKAN_RT_URL))?;
    if sha256(&zip)? != VULKAN_RT_ZIP_SHA256 {
        return Err("Vulkan runtime checksum mismatch".into());
    }
    run(Command::new(system32.join("tar.exe")).arg("-xf").arg(&zip).arg("-C").arg(&tmp))?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let from = tmp.join(VULKAN_RT_FOLDER);
    std::fs::copy(from.join("x64").join("vulkan-1.dll"), &dll).map_err(|e| e.to_string())?;
    std::fs::copy(from.join("VulkanRT-License.txt"), dir.join("VulkanRT-License.txt")).map_err(|e| e.to_string())?;
    if sha256(&dll)? != VULKAN_RT_DLL_SHA256 {
        return Err("vulkan-1.dll checksum mismatch".into());
    }
    Ok(())
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root().join("target"))
}

fn app_version() -> Result<String> {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).map_err(|e| e.to_string())?;
    manifest
        .lines()
        .find_map(|l| l.strip_prefix("version = \"").and_then(|v| v.strip_suffix('"')))
        .map(String::from)
        .ok_or_else(|| "no workspace version in Cargo.toml".into())
}

/// Removes Kikitori (Chidori's name before its rename) before installing: the install folder,
/// uninstall entry and shortcuts are named after the product, so an update from Kikitori would
/// leave it installed beside Chidori. A silent uninstall keeps the app data, which Chidori moves
/// to its own folders on first launch (core `migrate.rs`).
const REMOVE_KIKITORI: &str = r#"Section RemoveKikitori
  ReadRegStr $R8 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Kikitori" "InstallLocation"
  StrCpy $R9 $R8 1
  ${If} $R9 == '"'
    StrCpy $R8 $R8 -1 1
  ${EndIf}
  ${If} $R8 != ""
  ${AndIf} ${FileExists} "$R8\uninstall.exe"
    ExecWait '"$R8\uninstall.exe" /S _?=$R8'
    Delete "$R8\uninstall.exe"
    RMDir "$R8"
  ${EndIf}
SectionEnd"#;

/// cargo-packager's config, with absolute paths. Product name, identifier and publisher match the
/// installer the Tauri build made, so the new one installs over it (same folder, same uninstall
/// entry) and Tauri's updater can run it (`/P /UPDATE /R`).
fn packager_config(version: &str, release_dir: &Path, out_dir: &Path) -> serde_json::Value {
    let app = app_dir();
    let p = |rel: &str| app.join(rel).to_string_lossy().into_owned();
    serde_json::json!({
        "name": "chidori",
        "productName": "Chidori",
        "version": version,
        "identifier": "dev.yusai.chidori",
        "publisher": "yusai",
        "description": "Live transcription with local Whisper",
        "outDir": out_dir,
        "binariesDir": release_dir,
        "binaries": [{ "path": "chidori", "main": true }],
        "icons": [p("icons/32x32.png"), p("icons/128x128.png"), p("icons/128x128@2x.png"), p("icons/icon.ico")],
        "formats": ["nsis"],
        "resources": [
            { "src": p("resources/bench_ja.wav"), "target": "resources/bench_ja.wav" },
            { "src": p("resources/vulkan/vulkan-1.dll"), "target": "vulkan-1.dll" },
            { "src": p("resources/vulkan/VulkanRT-License.txt"), "target": "licenses/VulkanRT-License.txt" },
            { "src": p("resources/licenses/typst-LICENSE.txt"), "target": "licenses/typst-LICENSE.txt" },
            { "src": p("resources/licenses/typst-NOTICE.txt"), "target": "licenses/typst-NOTICE.txt" },
            { "src": p("resources/licenses/THIRD-PARTY-NOTICES.txt"), "target": "licenses/THIRD-PARTY-NOTICES.txt" }
        ],
        "nsis": {
            "installMode": "currentUser",
            "languages": ["Japanese", "English"],
            "installerIcon": p("icons/icon.ico"),
            "preinstallSection": REMOVE_KIKITORI,
            "appdataPaths": ["$APPDATA/$IDENTIFIER", "$LOCALAPPDATA/$IDENTIFIER"]
        }
    })
}

fn dist() -> Result<()> {
    if cfg!(target_os = "macos") {
        dist_mac()
    } else if cfg!(windows) {
        dist_windows()
    } else {
        Err("the installer is built on Windows or macOS".into())
    }
}

fn dist_windows() -> Result<()> {
    let version = app_version()?;
    let target = target_dir();
    ensure_vulkan_runtime()?;
    println!("Wrote {}", notices::write(&root())?.display());
    run(Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "chidori", "--features", "gpu-vulkan", "--locked"])
        .current_dir(root()))?;
    let release_dir = target.join("release");
    let out_dir = target.join("dist");
    let config = target.join("packager.json");
    std::fs::write(&config, packager_config(&version, &release_dir, &out_dir).to_string())
        .map_err(|e| e.to_string())?;
    run(Command::new("cargo-packager").args(["--release", "--config"]).arg(&config).current_dir(root()))?;
    let installer = std::fs::read_dir(&out_dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "exe") && p.to_string_lossy().contains(&version))
        .ok_or("cargo-packager made no installer")?;
    println!("Installer: {}", installer.display());
    let sig = PathBuf::from(format!("{}.sig", installer.display()));
    if sig.exists() {
        let signature = std::fs::read_to_string(&sig).map_err(|e| e.to_string())?;
        verify(&installer, &signature)?;
        if let (Ok(repo), Ok(tag)) = (std::env::var("GITHUB_REPOSITORY"), std::env::var("GITHUB_REF_NAME")) {
            let name = installer.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let manifest = serde_json::json!({
                "version": version,
                "pub_date": now_rfc3339(),
                "platforms": {
                    "windows-x86_64": {
                        "url": format!("https://github.com/{repo}/releases/download/{tag}/{name}"),
                        "signature": signature.trim(),
                    }
                }
            });
            let path = out_dir.join("latest.json");
            std::fs::write(&path, serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            println!("Wrote {}", path.display());
        }
    } else {
        println!("Not signed: set CARGO_PACKAGER_SIGN_PRIVATE_KEY for an installer the updater accepts");
    }
    Ok(())
}

/// The app's Info.plist keys beyond what cargo-packager writes: why it asks for the mic and the
/// system's sound (macOS refuses both without them).
const MAC_INFO_PLIST: &str = "macos/Info.plist";

/// Builds `Chidori_<version>_aarch64.dmg` in `<target>/dist`.
fn dist_mac() -> Result<()> {
    let version = app_version()?;
    let target = target_dir();
    println!("Wrote {}", notices::write(&root())?.display());
    let mut build = Command::new(env!("CARGO"));
    build.args(["build", "--release", "-p", "chidori", "--features", "gpu-metal", "--locked"]).current_dir(root());
    // `.cargo/config.toml`'s [env] carries MSVC flags for whisper.cpp; variables already set win
    // over it, so give clang the same optimization (scripts/macos-env.sh does this for cargo run).
    // Set them unconditionally: `cargo xtask` is `cargo run`, which hands this process those MSVC
    // flags already, so "only if unset" would pass `-nologo` on to clang.
    for (var, flags) in [
        ("CMAKE_C_FLAGS_RELEASE", "-O3 -DNDEBUG"),
        ("CMAKE_CXX_FLAGS_RELEASE", "-O3 -DNDEBUG"),
        ("CMAKE_C_FLAGS_RELWITHDEBINFO", "-O2 -g -DNDEBUG"),
        ("CMAKE_CXX_FLAGS_RELWITHDEBINFO", "-O2 -g -DNDEBUG"),
    ] {
        build.env(var, flags);
    }
    run(&mut build)?;

    let release_dir = target.join("release");
    let out_dir = target.join("dist");
    let _ = std::fs::remove_dir_all(&out_dir);
    let app = app_dir();
    let p = |rel: &str| app.join(rel).to_string_lossy().into_owned();
    let config = serde_json::json!({
        "name": "chidori",
        "productName": "Chidori",
        "version": version,
        "identifier": "dev.yusai.chidori",
        "publisher": "yusai",
        "description": "Live transcription with local Whisper",
        "outDir": out_dir,
        "binariesDir": release_dir,
        "binaries": [{ "path": "chidori", "main": true }],
        "icons": [p("icons/icon.icns")],
        "formats": ["app"],
        // host::Paths reads these from Contents/Resources.
        "resources": [
            { "src": p("resources/bench_ja.wav"), "target": "resources/bench_ja.wav" },
            { "src": p("resources/licenses/typst-LICENSE.txt"), "target": "licenses/typst-LICENSE.txt" },
            { "src": p("resources/licenses/typst-NOTICE.txt"), "target": "licenses/typst-NOTICE.txt" },
            { "src": p("resources/licenses/THIRD-PARTY-NOTICES.txt"), "target": "licenses/THIRD-PARTY-NOTICES.txt" }
        ],
        "macos": {
            "minimumSystemVersion": "14.6",
            "infoPlistPath": p(MAC_INFO_PLIST)
        }
    });
    let config_path = target.join("packager-macos.json");
    std::fs::write(&config_path, config.to_string()).map_err(|e| e.to_string())?;
    run(Command::new("cargo-packager").args(["--release", "--config"]).arg(&config_path).current_dir(root()))?;

    let bundle = out_dir.join("Chidori.app");
    if !bundle.is_dir() {
        return Err(format!("cargo-packager made no {}", bundle.display()));
    }
    // Ad hoc, without the hardened runtime (which would need entitlements for the mic). Unsigned
    // bundles are reported as "damaged" once downloaded; ad hoc ones get "Open Anyway".
    run(Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&bundle))?;
    run(Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(&bundle))?;

    // The window the .dmg opens: the app beside a link to /Applications to drag it onto.
    let stage = target.join("dmg-stage");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    run(Command::new("ditto").arg(&bundle).arg(stage.join("Chidori.app")))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("/Applications", stage.join("Applications")).map_err(|e| e.to_string())?;
    let dmg = out_dir.join(format!("Chidori_{version}_aarch64.dmg"));
    run(Command::new("hdiutil")
        .args(["create", "-volname", "Chidori", "-fs", "HFS+", "-format", "UDZO", "-ov", "-srcfolder"])
        .arg(&stage)
        .arg(&dmg))?;
    let _ = std::fs::remove_dir_all(&stage);
    println!("Disk image: {}", dmg.display());
    Ok(())
}

/// Checks an installer's signature against the key installed copies trust, so a release signed
/// with the wrong key fails here instead of on every user's PC.
fn verify(installer: &Path, signature_b64: &str) -> Result<()> {
    use base64::Engine as _;
    let text = |b64: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| "not base64 text".to_string())
    };
    let key = minisign_verify::PublicKey::decode(&text(include_str!("../../crates/core/src/update.pub"))?)
        .map_err(|e| format!("public key: {e}"))?;
    let signature = minisign_verify::Signature::decode(&text(signature_b64)?).map_err(|e| format!("signature: {e}"))?;
    let data = std::fs::read(installer).map_err(|e| e.to_string())?;
    key.verify(&data, &signature, true)
        .map_err(|e| format!("the installer's signature doesn't match the app's key: {e}"))?;
    println!("Signature checks out against the app's update key");
    Ok(())
}

/// The current UTC time as RFC 3339, without a date crate.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}
