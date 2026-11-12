//! Offline editor; opening it never loads credentials or calls a model.
use anyhow::{Result, ensure};
use std::{ffi::OsString, path::Path};

pub const USAGE: &str = "xjcode formula [--no-open]：离线公式编辑、预览和复制；将公式粘贴回 XJCode 输入框。";
pub const EDITOR: &str = include_str!("../assets/formula.html");

pub fn run(home: &Path, args: &[OsString]) -> Result<()> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    ensure!(args.is_empty() || (args.len() == 1 && args[0] == "--no-open"), "{USAGE}");
    let file = home.join("tools").join("formula.html");
    crate::auth::mark_home(home)?;
    crate::auth::atomic_write(&file, EDITOR.as_bytes())?;
    if args.is_empty() {
        open(&file)?;
    }
    println!("离线公式编辑器：{}\n复制公式后，粘贴到 XJCode 输入框。", file.display());
    Ok(())
}

#[cfg(windows)]
fn open(file: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let system = std::env::var_os("SystemRoot").ok_or_else(|| anyhow::anyhow!("找不到 Windows 系统目录"))?;
    let url = reqwest::Url::from_file_path(file).map_err(|_| anyhow::anyhow!("公式编辑器路径无效"))?;
    let status = std::process::Command::new(Path::new(&system).join("System32/rundll32.exe"))
        .arg("url.dll,FileProtocolHandler")
        .arg(url.as_str())
        .creation_flags(0x08000000)
        .status()?;
    ensure!(status.success(), "无法打开浏览器，请手动打开 {}", file.display());
    Ok(())
}

#[cfg(not(windows))]
fn open(_file: &Path) -> Result<()> {
    anyhow::bail!("请使用 --no-open 导出后在浏览器中打开")
}
