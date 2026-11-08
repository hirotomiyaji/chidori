//! The licence texts of everything the installer ships besides Chidori's own code: the Rust crates
//! linked into the exe, the whisper.cpp sources built from whisper-rs-sys, libopus built from
//! opusic-sys, and the fonts and icons the UI embeds. MIT, BSD, Apache-2.0 and the OFL ask for
//! their notices to travel with the binary. Typst's LICENSE and NOTICE and the Vulkan loader's
//! licence ship as their own files in the same licenses\ folder (the Vulkan one on Windows only).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::Result;

struct Entry {
    label: String,
    license: String,
    repository: Option<String>,
    files: Vec<PathBuf>,
}

/// LICENSE-MIT, license-apache-2.0, COPYING, COPYRIGHT, NOTICE, UNLICENSE and the like.
/// LICENSE.spdx files are machine-readable metadata, not licence texts.
fn license_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            ["licence", "license", "copying", "copyright", "notice", "unlicense"].iter().any(|s| name.starts_with(s))
                && !name.ends_with(".spdx")
        })
        .collect();
    files.sort();
    files
}

/// The build the installer ships on this OS: its GPU feature and target, as `cargo metadata` takes them.
fn shipped_build() -> (&'static str, &'static str) {
    if cfg!(target_os = "macos") {
        ("gpu-metal", "aarch64-apple-darwin")
    } else {
        ("gpu-vulkan", "x86_64-pc-windows-msvc")
    }
}

/// Crates linked into the exe of the GPU build this OS ships. Build scripts and proc macros only
/// run while compiling, so crates that only they depend on are left out.
fn rust_crates(root: &Path) -> Result<Vec<Value>> {
    let (feature, platform) = shipped_build();
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--features", feature])
        .args(["--filter-platform", platform, "--manifest-path"])
        .arg(root.join("crates").join("app").join("Cargo.toml"))
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let packages: HashMap<&str, &Value> =
        meta["packages"].as_array().into_iter().flatten().map(|p| (p["id"].as_str().unwrap_or_default(), p)).collect();
    let nodes: HashMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| (n["id"].as_str().unwrap_or_default(), n))
        .collect();
    let app_id = packages
        .values()
        .find(|p| p["name"] == "chidori")
        .and_then(|p| p["id"].as_str())
        .ok_or("no chidori package")?;
    let mut seen: HashSet<&str> = HashSet::from([app_id]);
    let mut queue = vec![app_id];
    let mut crates = Vec::new();
    while let Some(id) = queue.pop() {
        for dep in nodes[id]["deps"].as_array().into_iter().flatten() {
            let pkg = dep["pkg"].as_str().unwrap_or_default();
            let normal = dep["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
            if seen.contains(pkg) || !normal {
                continue;
            }
            seen.insert(pkg);
            let p = packages[pkg];
            let proc_macro = p["targets"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|t| t["kind"].as_array().into_iter().flatten().any(|k| k == "proc-macro"));
            if proc_macro {
                continue;
            }
            // Chidori's own crates are under its own licence.
            if p["name"] != "chidori-core" {
                crates.push(p.clone());
            }
            queue.push(pkg);
        }
    }
    Ok(crates)
}

fn crate_dir(p: &Value) -> PathBuf {
    Path::new(p["manifest_path"].as_str().unwrap_or_default()).parent().map(PathBuf::from).unwrap_or_default()
}

fn crate_entry(p: &Value) -> Entry {
    let dir = crate_dir(p);
    let mut files = license_files(&dir);
    if let Some(declared) = p["license_file"].as_str().map(|f| dir.join(f))
        && declared.exists()
        && !files.contains(&declared)
    {
        files.push(declared);
    }
    Entry {
        label: format!("{} {}", p["name"].as_str().unwrap_or_default(), p["version"].as_str().unwrap_or_default()),
        license: p["license"].as_str().unwrap_or("unknown").to_string(),
        repository: p["repository"].as_str().map(String::from),
        files,
    }
}

/// A source tree a `-sys` crate compiles, with the licence it carries.
fn vendored(crates: &[Value], sys: &str, file: &[&str], label: &str, license: &str) -> Result<Entry> {
    let p = crates.iter().find(|p| p["name"] == sys).ok_or(format!("{sys} is not a dependency"))?;
    let path = file.iter().fold(crate_dir(p), |d, f| d.join(f));
    if !path.exists() {
        return Err(format!("{} is missing: check where {sys} keeps its sources", path.display()));
    }
    let version = p["version"].as_str().unwrap_or_default();
    Ok(Entry {
        label: format!("{label} (built from {sys} {version})"),
        license: license.into(),
        repository: None,
        files: vec![path],
    })
}

fn normalize(text: &str) -> String {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string()
}

/// "a 1.0, b 2.0, ..." broken into lines of at most 80 characters.
fn wrap_list(items: &[String]) -> String {
    let mut lines = vec![String::new()];
    for item in items {
        let line = lines.last_mut().expect("one line");
        if !line.is_empty() && line.len() + 2 + item.len() > 80 {
            line.push(',');
            lines.push(item.clone());
        } else if line.is_empty() {
            line.push_str(item);
        } else {
            line.push_str(", ");
            line.push_str(item);
        }
    }
    lines.join("\n")
}

const RULE: &str = "--------------------------------------------------------------------------------";
const DOUBLE: &str = "================================================================================";

/// One block per distinct licence text, listing every package that ships it, then the packages
/// that ship no licence file at all.
fn section(title: &str, entries: &[Entry]) -> Result<String> {
    let mut texts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in entries {
        for file in &entry.files {
            let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
            texts.entry(normalize(&text)).or_default().insert(entry.label.clone());
        }
    }
    let mut blocks: Vec<(Vec<String>, String)> = texts.into_iter().map(|(t, l)| (l.into_iter().collect(), t)).collect();
    blocks.sort_by(|a, b| a.0[0].cmp(&b.0[0]));
    let mut out = vec![format!("{DOUBLE}\n{title}\n{DOUBLE}")];
    for (labels, text) in blocks {
        out.push(format!("{RULE}\nUsed by: {}\n\n{text}", wrap_list(&labels)));
    }
    let mut bare: Vec<&Entry> = entries.iter().filter(|e| e.files.is_empty()).collect();
    bare.sort_by(|a, b| a.label.cmp(&b.label));
    if !bare.is_empty() {
        let lines: Vec<String> = bare
            .iter()
            .map(|e| match &e.repository {
                Some(r) => format!("{}: {}, {r}", e.label, e.license),
                None => format!("{}: {}", e.label, e.license),
            })
            .collect();
        out.push(format!(
            "{RULE}\nThese packages ship no licence file. Each line gives the licence it declares and\nwhere its source is.\n\n{}",
            lines.join("\n")
        ));
    }
    Ok(out.join("\n\n"))
}

pub fn notices(root: &Path) -> Result<String> {
    let crates = rust_crates(root)?;
    let brand = root.join("crates").join("app").join("brand");
    let header = "Third-party software in Chidori
================================

Chidori itself is under the MIT licence. Its installer also contains the software below; each
licence text is followed by the packages it covers. Typst's licence and NOTICE (and on Windows
the Vulkan loader's licence) are the other files in this folder.";
    let ui = vec![
        Entry {
            label: "Zen Kaku Gothic New".into(),
            license: "OFL-1.1".into(),
            repository: None,
            files: vec![brand.join("fonts").join("OFL-ZenKakuGothicNew.txt")],
        },
        Entry {
            label: "IBM Plex Mono".into(),
            license: "OFL-1.1".into(),
            repository: None,
            files: vec![brand.join("fonts").join("OFL-IBMPlexMono.txt")],
        },
        Entry {
            label: "Lucide icons".into(),
            license: "ISC".into(),
            repository: None,
            files: vec![brand.join("icons").join("LUCIDE-LICENSE")],
        },
    ];
    let parts = [
        header.to_string(),
        section(
            "whisper.cpp",
            &[vendored(&crates, "whisper-rs-sys", &["whisper.cpp", "LICENSE"], "whisper.cpp and ggml", "MIT")?],
        )?,
        section("libopus", &[vendored(&crates, "opusic-sys", &["opus", "COPYING"], "libopus", "BSD-3-Clause")?])?,
        section("Rust crates", &crates.iter().map(crate_entry).collect::<Vec<_>>())?,
        section("Fonts and icons", &ui)?,
    ];
    Ok(parts.join("\n\n\n") + "\n")
}

/// Writes `crates/app/resources/licenses/THIRD-PARTY-NOTICES.txt` (git-ignored).
pub fn write(root: &Path) -> Result<PathBuf> {
    let out = root.join("crates").join("app").join("resources").join("licenses").join("THIRD-PARTY-NOTICES.txt");
    std::fs::write(&out, notices(root)?).map_err(|e| e.to_string())?;
    Ok(out)
}
