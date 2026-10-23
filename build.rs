//! 把图标和版本信息编进可执行文件。
//!
//! 图标由 `src/platform/logo.rs` 画出来——和托盘、窗口用的是同一份描述——所以没有图片要同步。
//! 唯一额外的环节是 Windows SDK 里的资源编译器；找不到它构建也照常成功，只是资源
//! 管理器里那个文件图标会缺。

use std::path::{Path, PathBuf};
use std::process::Command;

// 这里只需要 `ico()`，托盘用的那套函数在构建脚本里用不上。
#[allow(dead_code)]
#[path = "src/platform/logo.rs"]
mod logo;

fn main() {
    println!("cargo:rerun-if-changed=src/platform/logo.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR 一定存在"));
    let icon = out_dir.join("gpui-tools.ico");
    if let Err(error) = std::fs::write(&icon, logo::ico()) {
        println!("cargo:warning=写不出生成的图标：{error}");
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let Some(compiler) = find_resource_compiler() else {
        println!("cargo:warning=找不到 rc.exe，可执行文件保留默认图标");
        return;
    };
    let script = out_dir.join("gpui-tools.rc");
    if let Err(error) = std::fs::write(&script, script_for(&icon)) {
        println!("cargo:warning=写不出资源脚本：{error}");
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
        Ok(status) => println!("cargo:warning=rc.exe 失败，状态 {status}"),
        Err(error) => println!("cargo:warning=运行不了 rc.exe：{error}"),
    }
}

/// 资源脚本：id 1 下的图标（窗口按编号取它），加上资源管理器显示的版本块。
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

/// 找资源编译器：先看 `PATH`，再看磁盘上最新的 Windows SDK。
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
    // 新的排前面：一台机器可能装了好几个 SDK，最新那个才是当前工具链对应的。
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

/// 把 `10.0.26100.0` 这种 SDK 目录名变成可比较的数字序列。
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
