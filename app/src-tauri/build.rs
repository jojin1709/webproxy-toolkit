fn main() {
    tauri_build::build();

    // `cargo test` binaries link the same Tauri code as the app — including an
    // import of `TaskDialogIndirect` from comctl32 — but tauri-build only
    // embeds the comctl32 v6 activation manifest into the application
    // executable. Without it the loader picks the v5 comctl32 in System32,
    // which has no such export, and the test process dies with
    // 0xC0000139 (STATUS_ENTRYPOINT_NOT_FOUND) before main() runs.
    //
    // Cargo has no test-only link argument (`rustc-link-arg-tests` is not
    // honoured), so the manifest is moved out of tauri's resource file — which
    // tauri-build links into `bins` only — and re-added through
    // `/MANIFESTINPUT`, which reaches every binary of this package: tests get
    // their missing manifest, bins keep exactly the one they had.
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        hand_manifest_to_linker();
    }
    println!("cargo:rerun-if-changed=build.rs");
}

fn hand_manifest_to_linker() {
    let out_dir = match std::env::var("OUT_DIR") {
        Ok(dir) => std::path::PathBuf::from(dir),
        Err(_) => return,
    };
    let rc_path = out_dir.join("resource.rc");
    let rc_text = match std::fs::read_to_string(&rc_path) {
        Ok(text) => text,
        Err(err) => {
            println!("cargo:warning=resource.rc unreadable, test binaries may crash on startup: {err}");
            return;
        }
    };
    let (rc_without_manifest, manifest_xml) = match split_manifest_resource(&rc_text) {
        Some(parts) => parts,
        None => {
            panic!(
                "could not locate the comctl32 v6 manifest in `{}`; \
                 without handling it, `cargo test` dies with 0xC0000139",
                rc_path.display()
            );
        }
    };

    let manifest_path = out_dir.join("common-controls.manifest");
    let resource_lib = out_dir.join("resource.lib");
    if let Err(err) = std::fs::write(&manifest_path, &manifest_xml) {
        println!("cargo:warning=cannot write manifest, test binaries may crash on startup: {err}");
        return;
    }
    if let Err(err) = compile_resources(&out_dir, &rc_path, &rc_without_manifest, &resource_lib) {
        panic!("failed to recompile `{}` without its manifest: {err}", rc_path.display());
    }

    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest_path.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
}

/// Splits the `1 24 { ... }` (RT_MANIFEST) resource out of an `.rc` file.
/// Returns the remaining resource script and the manifest XML it carried.
fn split_manifest_resource(rc: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = rc.lines().collect();
    let start = lines.iter().position(|line| line.trim() == "1 24")?;
    if lines.get(start + 1).map(|next| next.trim()) != Some("{") {
        return None;
    }

    let mut depth = 1usize;
    let mut end = None;
    for (index, line) in lines.iter().enumerate().skip(start + 2) {
        let code = strip_rc_strings(line);
        let opens = code.matches('{').count();
        let closes = code.matches('}').count();
        depth = (depth + opens).saturating_sub(closes);
        if depth == 0 {
            end = Some(index);
            break;
        }
    }
    let end = end?;
    let block = &lines[start + 2..end];

    let mut xml = String::new();
    for line in block {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix('"') else { continue };
        let Some(rest) = rest.strip_suffix('"') else { continue };
        xml.push_str(&rest.replace("\"\"", "\""));
        xml.push('\n');
    }
    if !xml.contains("<assembly") {
        return None;
    }

    let mut remaining = String::new();
    for (index, line) in lines.iter().enumerate() {
        if (start..=end).contains(&index) {
            continue;
        }
        remaining.push_str(line);
        remaining.push('\n');
    }
    Some((remaining, xml))
}

/// Removes double-quoted spans so brace counting only sees resource syntax.
fn strip_rc_strings(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_string && ch == '"' {
            if chars.peek() == Some(&'"') {
                chars.next();
            } else {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        if !in_string {
            out.push(ch);
        }
    }
    out
}

fn compile_resources(
    out_dir: &std::path::Path,
    rc_path: &std::path::Path,
    rc_text: &str,
    resource_lib: &std::path::Path,
) -> Result<(), String> {
    std::fs::write(rc_path, rc_text).map_err(|err| format!("writing stripped rc: {err}"))?;
    let rc_exe = find_rc_exe().ok_or_else(|| "rc.exe not found (PATH, RC env, or Windows Kits)".to_string())?;
    let output = std::process::Command::new(&rc_exe)
        .args(["/nologo", "/fo"])
        .arg(resource_lib)
        .arg("/I")
        .arg(out_dir)
        .arg(rc_path)
        .output()
        .map_err(|err| format!("running {}: {err}", rc_exe.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            rc_exe.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn find_rc_exe() -> Option<std::path::PathBuf> {
    for key in ["RC", "RC_TARGET"] {
        if let Ok(value) = std::env::var(key) {
            let path = std::path::PathBuf::from(value);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    if let Some(triple) = std::env::var("CARGO_CFG_TARGET_TRIPLE").ok() {
        let key = format!("RC_{}", triple.replace('-', "_"));
        if let Ok(value) = std::env::var(&key) {
            let path = std::path::PathBuf::from(value);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    for root in kits_roots() {
        let bin = root.join("bin");
        let Ok(entries) = std::fs::read_dir(&bin) else { continue };
        let mut versions: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .collect();
        versions.sort_by_key(|entry| entry.file_name());
        versions.reverse();
        for entry in versions {
            for candidate in ["x64", "x86", ""] {
                let mut path = entry.path();
                if !candidate.is_empty() {
                    path = path.join(candidate);
                }
                path = path.join("rc.exe");
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    for dir in std::env::var_os("PATH")?.to_string_lossy().split(';') {
        let path = std::path::Path::new(dir).join("rc.exe");
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn kits_roots() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    for key in [
        r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots",
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots",
    ] {
        if let Ok(output) = std::process::Command::new("reg")
            .args(["query", key, "/v", "KitsRoot10"])
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if let Some(path) = line.split("REG_SZ").nth(1) {
                        let path = path.trim();
                        if !path.is_empty() {
                            roots.push(std::path::PathBuf::from(path));
                        }
                    }
                }
            }
        }
    }
    roots
}
