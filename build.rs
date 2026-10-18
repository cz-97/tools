//! Embeds the application's icon and version block into the executable.
//!
//! The icon is generated from `src/logo.rs` — the same description the tray and
//! the window use — so there is no image file to keep in sync. The only extra
//! moving part is the resource compiler that ships with the Windows SDK; when
//! it cannot be found, the build still succeeds and only the file icon in
//! Explorer goes missing.

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "src/logo.rs"]
mod logo;

fn main() {
    println!("cargo:rerun-if-changed=src/logo.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let icon = out_dir.join("gpui-tools.ico");
    if let Err(error) = std::fs::write(&icon, logo::ico()) {
        println!("cargo:warning=could not write the generated icon: {error}");
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let Some(compiler) = find_resource_compiler() else {
        println!("cargo:warning=rc.exe was not found; the executable keeps its default icon");
        return;
    };
    let script = out_dir.join("gpui-tools.rc");
    if let Err(error) = std::fs::write(&script, script_for(&icon)) {
        println!("cargo:warning=could not write the resource script: {error}");
        return;
    }

    let resource = out_dir.join("gpui-tools.res");
    let status = Command::new(&compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&script)
        .status();
    match status {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg-bins={}", resource.display());
        }
        Ok(status) => println!("cargo:warning=rc.exe failed with status {status}"),
        Err(error) => println!("cargo:warning=could not run rc.exe: {error}"),
    }
}

/// The resource script: the icon under id 1, which the window looks up by
/// number, plus the version block Explorer shows on the file.
fn script_for(icon: &Path) -> String {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let mut fields: Vec<u16> = version
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .collect();
    fields.resize(4, 0);
    let numbers = fields
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let icon = icon.display().to_string().replace('\\', "/");

    format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "GPUI Tools launcher"
      VALUE "ProductName", "GPUI Tools"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "gpui-tools.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}

/// The resource compiler, from `PATH` or from the newest Windows SDK on disk.
fn find_resource_compiler() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let kits =
        std::env::var_os("ProgramFiles(x86)").or_else(|| std::env::var_os("ProgramFiles"))?;
    let bin = PathBuf::from(kits)
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut versions = std::fs::read_dir(&bin)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    // Newest first: a machine can carry several SDKs, and the newest one is the
    // one the installed toolchain was built against.
    versions.sort_by_key(|path| version_key(path));
    versions.reverse();

    versions
        .into_iter()
        .flat_map(|version| {
            ["x64", "arm64", "x86"]
                .map(|arch| version.join(arch).join("rc.exe"))
                .into_iter()
        })
        .find(|candidate| candidate.is_file())
}

/// A sortable number for an SDK folder such as `10.0.26100.0`.
fn version_key(path: &Path) -> Vec<u16> {
    let mut key = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .collect::<Vec<_>>();
    key.resize(4, 0);
    key
}
