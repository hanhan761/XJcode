use crate::updates;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};
#[derive(Debug)]
pub struct Installation {
    pub kind: &'static str,
    pub root: PathBuf,
    pub node: PathBuf,
    pub npm: PathBuf,
    pub no_path: bool,
    pub private_node: bool,
}
pub fn installation() -> Result<Installation> {
    let exe = std::env::current_exe()?.canonicalize()?;
    for root in exe.ancestors().skip(1) {
        let file = root.join("installation.json");
        if file.is_file() {
            let data = std::fs::read(file)?;
            let state: Value =
                serde_json::from_slice(data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data))?;
            ensure!(
                state["Schema"] == 1 && state["Product"] == "XJCode",
                "安装归属记录无效"
            );
            let saved =
                PathBuf::from(state["Root"].as_str().context("缺少安装路径")?).canonicalize()?;
            ensure!(
                saved == root && exe.starts_with(root.join("apps")),
                "安装记录与程序位置不一致"
            );
            return Ok(Installation {
                kind: "managed",
                root: portable_path(root),
                node: PathBuf::from(state["Node"].as_str().context("缺少运行时")?),
                npm: PathBuf::from(state["Npm"].as_str().context("缺少 npm")?),
                no_path: state["NoPath"].as_bool().unwrap_or(false),
                private_node: state["PrivateNode"].as_bool().unwrap_or(false),
            });
        }
    }
    if let Some(prefix) = std::env::var_os("XJCODE_NPM_PREFIX") {
        let root = PathBuf::from(prefix).canonicalize()?;
        ensure!(
            root.join("node_modules/xjcode/native/xjcode.exe")
                .canonicalize()?
                == exe,
            "npm 安装来源不一致"
        );
        let node = PathBuf::from(std::env::var_os("XJCODE_NODE").context("无法定位 Node")?);
        let npm = node
            .parent()
            .context("无效 Node 路径")?
            .join("node_modules/npm/bin/npm-cli.js");
        ensure!(
            node.is_file() && npm.is_file(),
            "无法定位原 npm 运行时，请使用官网安装器迁移"
        );
        return Ok(Installation {
            kind: "npm",
            root: portable_path(&root),
            node,
            npm,
            no_path: true,
            private_node: false,
        });
    }
    bail!(
        "此安装缺少可靠归属记录；请先运行一次官网新版安装器完成迁移： https://xjcode.wenshanzhike.com/"
    )
}
fn portable_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(p) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{p}"))
    } else {
        PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s))
    }
}
fn powershell() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("SystemRoot").context("找不到 Windows 目录")?)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
}
fn job(dir: &Path, value: &Value) -> Result<()> {
    std::fs::write(
        dir.join("manage-xjcode.ps1"),
        include_str!("../../scripts/manage-xjcode.ps1"),
    )?;
    std::fs::write(dir.join("request.json"), serde_json::to_vec(value)?)?;
    Ok(())
}
fn command(dir: &Path) -> Result<std::process::Command> {
    let mut c = std::process::Command::new(powershell()?);
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ])
    .arg(dir.join("manage-xjcode.ps1"))
    .arg("-JobFile")
    .arg(dir.join("request.json"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    Ok(c)
}
fn deferred(dir: tempfile::TempDir, value: Value) -> Result<()> {
    let dir = dir.keep();
    job(&dir, &value)?;
    let log = dir.join("result.log");
    let f = std::fs::File::create(&log)?;
    command(&dir)?
        .stdin(std::process::Stdio::null())
        .stdout(f.try_clone()?)
        .stderr(f)
        .spawn()?;
    println!(
        "维护操作将在当前命令退出后执行。结果日志：{}",
        log.display()
    );
    Ok(())
}
pub async fn update(home: &Path, args: &[OsString]) -> Result<()> {
    ensure!(
        args.is_empty() || (args.len() == 1 && args[0] == "--check"),
        "用法：xjcode update [--check]"
    );
    println!("正在检查 XJCode 更新…");
    let latest = updates::check(home).await?;
    if updates::version(&latest.version)? <= updates::version(updates::VERSION)? {
        println!("已是最新版本：{}", updates::VERSION);
        return Ok(());
    }
    println!("发现新版：{} → {}", updates::VERSION, latest.version);
    if !args.is_empty() {
        println!("运行 xjcode update 安装新版。");
        return Ok(());
    }
    let install = installation()?;
    let dir = tempfile::Builder::new()
        .prefix("xjcode-maintenance-")
        .tempdir()?;
    let base = format!("{}/releases/{}/", updates::ORIGIN, latest.version);
    let client = updates::client(600)?;
    let bytes = updates::download(&client, &format!("{base}release-manifest.json"), 65536).await?;
    ensure!(
        hex::encode(Sha256::digest(&bytes)).eq_ignore_ascii_case(&latest.manifest_sha256),
        "发行清单校验失败，未修改原安装"
    );
    let manifest: Value = serde_json::from_slice(&bytes)?;
    ensure!(manifest["version"] == latest.version, "发行清单版本不一致");
    std::fs::write(dir.path().join("release-manifest.json"), &bytes)?;
    for key in if install.kind == "managed" {
        vec!["npm", "bootstrap", "node"]
    } else {
        vec!["npm"]
    } {
        let a = &manifest[key];
        let name = a["file"].as_str().context("发行文件信息缺失")?;
        ensure!(
            name != "."
                && name != ".."
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "发行文件名无效"
        );
        if key == "node"
            && (!install.private_node
                || install
                    .root
                    .join("runtime")
                    .join(format!(
                        "node-{}-win-x64",
                        a["version"].as_str().unwrap_or("")
                    ))
                    .join("node.exe")
                    .is_file())
        {
            continue;
        }
        println!("下载并校验：{name}");
        updates::artifact(
            &client,
            &format!("{base}{name}"),
            a["sha256"].as_str().context("缺少文件校验值")?,
            &dir.path().join(name),
            1024 * 1024 * 1024,
        )
        .await?;
    }
    let mut value = json!({"Action":if install.kind=="managed"{"UpdateManaged"}else{"UpdateNpm"},"Kind":install.kind,"Root":install.root,"Node":install.node,"Npm":install.npm,"NoPath":install.no_path,"PrivateNode":install.private_node,"Version":latest.version,"Package":dir.path().join(manifest["npm"]["file"].as_str().unwrap()),"PackageHash":manifest["npm"]["sha256"],"Gui":false,"WaitPids":[]});
    if install.kind == "managed" {
        value["BundleRoot"] = json!(dir.path());
        value["Bootstrap"] = json!(
            dir.path().join(
                manifest["bootstrap"]["file"]
                    .as_str()
                    .context("缺少安装脚本")?
            )
        );
        value["BootstrapHash"] = manifest["bootstrap"]["sha256"].clone();
        job(dir.path(), &value)?;
        ensure!(
            command(dir.path())?.status()?.success(),
            "更新失败，原版本保持可用"
        );
    } else {
        if let Some(request_dir) = std::env::var_os("XJCODE_JOB_DIR") {
            // The npm wrapper waits for this native process to exit before replacing it.
            value["BundleRoot"] = json!(dir.path());
            let kept = dir.keep();
            value["Cleanup"] = json!(kept);
            job(Path::new(&request_dir), &value)?;
            return Ok(());
        }
        value["WaitPids"] = json!([
            std::process::id(),
            std::env::var("XJCODE_PARENT_PID")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0)
        ]);
        return deferred(dir, value);
    }
    println!("XJCode 已更新到 {}，请重新启动已有会话。", latest.version);
    Ok(())
}
pub fn uninstall(home: &Path, args: &[OsString]) -> Result<()> {
    let mut yes = false;
    let mut purge = false;
    for a in args {
        match a.to_str() {
            Some("--yes") => yes = true,
            Some("--purge") => purge = true,
            _ => bail!("用法：xjcode uninstall [--purge] [--yes]"),
        }
    }
    let install = installation()?;
    println!(
        "卸载目录：{}\n个人数据：{}",
        install.root.display(),
        if purge {
            "清除登录、配置与会话"
        } else {
            "保留"
        }
    );
    if !yes {
        ensure!(io::stdin().is_terminal(), "无交互卸载须显式指定 --yes");
        print!("确认卸载？输入 yes：");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        ensure!(line.trim() == "yes", "已取消卸载");
    }
    let value = json!({"Action":"Uninstall","Kind":install.kind,"Root":install.root,"Node":install.node,"Npm":install.npm,"Home":home,"Purge":purge,"Gui":false,"WaitPids":[std::process::id(),std::env::var("XJCODE_PARENT_PID").ok().and_then(|s|s.parse::<u32>().ok()).unwrap_or(0)]});
    deferred(
        tempfile::Builder::new()
            .prefix("xjcode-maintenance-")
            .tempdir()?,
        value,
    )
}
