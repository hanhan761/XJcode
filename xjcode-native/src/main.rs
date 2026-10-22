use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::OsString,
    io::{self, Write},
    path::Path,
    time::Duration,
};
use xjcode_native::{
    api::TopClient,
    auth,
    setup::{self, Settings},
};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("XJCode: {e:#}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let first = args.first().and_then(|a| a.to_str()).unwrap_or("");
    let home = auth::home()?;
    match first {
        "--version" | "-V" => {
            println!("xjcode {} · 交晓智", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        "--help" | "-h" | "help" => {
            println!(
                "XJCode — 使用交晓智的本地编程工具\n\nxjcode login [--force]     西交登录，自动配置凭据\nxjcode login status        登录状态\nxjcode logout              清除本机凭据\nxjcode models [--refresh]  列出授权模型\nxjcode models --select    选择默认模型\nxjcode doctor [--probe] [-m 模型]  环境诊断（--probe 会调用一次指定模型）\nxjcode                    交互编程\nxjcode exec \"任务\"         单次任务\nxjcode resume             恢复会话\nxjcode mcp                管理本地 MCP 扩展（会话内使用 /skills）\n\n命令详情：xjcode exec --help。默认上下文预算 32768 tokens，可在 config.toml 中调整。"
            );
            return Ok(());
        }
        "login" => {
            if args.get(1).and_then(|x| x.to_str()) == Some("status") {
                let c = auth::load_at(&home)?;
                let s = setup::load(&home)?;
                ensure!(c.account_id == s.account_id, "账号与配置不一致");
                println!(
                    "已保存登录凭据（未在线校验）\n工作区：{}\n默认模型：{}",
                    s.workspace_name, s.model
                );
                return Ok(());
            }
            ensure!(
                args.len() == 1 || (args.len() == 2 && args[1] == "--force"),
                "用法：xjcode login [--force|status]"
            );
            let credentials = if args.len() == 1 {
                match auth::load_at(&home) {
                    Ok(c) => {
                        let client = TopClient::new(c.clone())?;
                        match client.identity().await {
                            Ok(id) if id == c.account_id => c,
                            _ => browser_login().await?,
                        }
                    }
                    Err(_) => browser_login().await?,
                }
            } else {
                browser_login().await?
            };
            configure(&home, credentials).await?;
            return Ok(());
        }
        "logout" => {
            ensure!(args.len() == 1, "用法：xjcode logout");
            auth::logout_at(&home)?;
            println!("本机凭据已清除；服务器密钥保持有效。");
            return Ok(());
        }
        "models" => {
            let mut settings = setup::load(&home)?;
            if args.iter().any(|a| a == "--refresh") {
                // TUI selection updates config.toml independently of the login
                // snapshot. Refresh capabilities without restoring an old model.
                settings.model = xjcode_native::doctor::Options::default().model_name(&home, &settings)?;
                let c = auth::load_at(&home)?;
                ensure!(c.account_id == settings.account_id, "账号不一致");
                settings.models = TopClient::new(c)?.models(&settings.workspace_id).await?;
                ensure!(!settings.models.is_empty(), "没有可用模型");
                if !settings.models.iter().any(|m| m.name == settings.model) {
                    bail!("默认模型已不可用，请重新运行 xjcode login 选择模型");
                }
                setup::save(&home, &settings)?;
            }
            if args.iter().any(|a| a == "--select") {
                let labels = settings
                    .models
                    .iter()
                    .map(|m| m.name.clone())
                    .collect::<Vec<_>>();
                settings.model = settings.models[select("模型", &labels)?].name.clone();
                setup::save(&home, &settings)?;
            }
            xjcode_native::capabilities::apply(&home, &mut settings);
            for model in settings.models {
                println!(
                    "{} {}{}",
                    if model.name == settings.model {
                        "*"
                    } else {
                        " "
                    },
                    model.name,
                    if model.verified_image_input { "  [图片]" } else if model.platform_supports_images() { "  [图片·平台声明]" } else { "" }
                );
            }
            return Ok(());
        }
        "doctor" => {
            if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
                println!("{}", xjcode_native::doctor::USAGE);
                return Ok(());
            }
            let options = xjcode_native::doctor::Options::parse(&args[1..])?;
            return doctor(&home, options).await;
        }
        "cloud" | "app" | "update" | "remote-control" | "app-server" | "agents"
        | "login-with-api-key" => bail!("此入口依赖其他后端，XJCode 第一版不提供"),
        _ => {}
    }
    let exe = std::env::current_exe()?.with_file_name("xjcode-engine.exe");
    ensure!(exe.is_file(), "缺少 xjcode-engine.exe，请安装完整发行包");
    let is_help = args.iter().any(|a| a == "--help" || a == "-h");
    let mut cmd = std::process::Command::new(exe);
    cmd.env("CODEX_HOME", &home)
        .env("XJCODE_HOME", &home)
        .env("CODEX_SQLITE_HOME", &home)
        .env("XJCODE_PROVIDER_LOCK", "1");
    for key in [
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
    ] {
        cmd.env_remove(key);
    }
    if !is_help {
        let c = auth::load_at(&home)?;
        let settings = setup::load(&home)?;
        ensure!(
            settings.account_id == c.account_id,
            "账号不一致，请重新登录"
        );
        #[cfg(windows)]
        configure_python_environment(&mut cmd);
    }
    cmd.args(&args);
    if is_help {
        // Help is generated locally and contains no user or model content.
        let output = cmd.output().context("无法读取命令帮助")?;
        let brand = |bytes: &[u8]| {
            String::from_utf8_lossy(bytes)
                .replace("OpenAI Codex", "XJCode")
                .replace("Codex", "XJCode")
                .replace("codex", "xjcode")
        };
        print!("{}", brand(&output.stdout));
        eprint!("{}", brand(&output.stderr));
        std::process::exit(output.status.code().unwrap_or(1));
    }
    // Preserve the user's terminal; this is the interactive engine, not a background shell.
    let status = cmd.status().context("无法启动 XJCode 编程引擎")?;
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(windows)]
fn configure_python_environment(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;

    // Captured/background Python stdout otherwise waits in a block buffer,
    // hiding progress even while a training process is actively computing.
    command.env("PYTHONUNBUFFERED", "1");

    fn usable_python(path: &Path) -> bool {
        path.is_absolute()
            && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.eq_ignore_ascii_case("python.exe"))
            && !path.to_string_lossy().to_ascii_lowercase().contains("\\microsoft\\windowsapps\\")
            && std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    }

    let search_paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let python = std::env::var_os("XJCODE_PYTHON")
        .map(PathBuf::from)
        .filter(|path| usable_python(path))
        .or_else(|| search_paths.iter().map(|dir| dir.join("python.exe")).find(|path| usable_python(path)))
        .or_else(|| {
            let launcher = search_paths.iter().map(|dir| dir.join("py.exe")).find(|path| {
                !path.to_string_lossy().to_ascii_lowercase().contains("\\microsoft\\windowsapps\\")
                    && std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
            })?;
            // Query only interpreter identity. Isolated mode skips project and
            // user startup code; no task scripts or solver jobs are executed.
            let output = std::process::Command::new(launcher)
                .args(["-3", "-I", "-S", "-c", "import sys; sys.stdout.buffer.write(sys.executable.encode('utf-8'))"])
                .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
                .output().ok()?;
            if !output.status.success() { return None; }
            let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
            usable_python(&path).then_some(path)
        });
    let Some(python) = python else { return };
    let Some(parent) = python.parent() else { return };
    let mut child_paths = vec![parent.to_path_buf()];
    child_paths.extend(search_paths.into_iter().filter(|path| path != parent));
    if let Ok(path) = std::env::join_paths(child_paths) {
        command.env("PATH", path).env("XJCODE_PYTHON", python);
    }
}

fn select(label: &str, options: &[String]) -> Result<usize> {
    ensure!(!options.is_empty(), "没有可用{label}");
    if options.len() == 1 {
        return Ok(0);
    }
    println!("请选择{label}：");
    for (i, name) in options.iter().enumerate() {
        println!("  {}. {}", i + 1, name);
    }
    loop {
        print!("输入编号：");
        io::stdout().flush()?;
        let mut line = String::new();
        ensure!(io::stdin().read_line(&mut line)? > 0, "选择已取消");
        if let Ok(n) = line.trim().parse::<usize>() {
            if n >= 1 && n <= options.len() {
                return Ok(n - 1);
            }
        }
        println!("编号无效。");
    }
}
async fn configure(home: &Path, credentials: auth::Credentials) -> Result<()> {
    let client = TopClient::new(credentials.clone())?;
    ensure!(
        client.identity().await? == credentials.account_id,
        "网页登录与 API 密钥不属于同一账号，未保存"
    );
    let workspaces = client.workspaces().await?;
    let labels = workspaces
        .iter()
        .map(|w| w.name.clone())
        .collect::<Vec<_>>();
    let previous = setup::load(home)
        .ok()
        .filter(|s| s.account_id == credentials.account_id);
    let index = previous
        .as_ref()
        .and_then(|s| workspaces.iter().position(|w| w.id == s.workspace_id))
        .map(Ok)
        .unwrap_or_else(|| select("工作区", &labels))?;
    let workspace = &workspaces[index];
    let models = client.models(&workspace.id).await?;
    let labels = models.iter().map(|m| m.name.clone()).collect::<Vec<_>>();
    let index = previous
        .as_ref()
        .and_then(|s| models.iter().position(|m| m.name == s.model))
        .map(Ok)
        .unwrap_or_else(|| select("模型", &labels))?;
    let settings = Settings {
        account_id: credentials.account_id.clone(),
        workspace_id: workspace.id.clone(),
        workspace_name: workspace.name.clone(),
        model: models[index].name.clone(),
        models,
    };
    setup::save(home, &settings)?;
    auth::save_at(home, &credentials)?;
    println!(
        "登录完成。工作区：{}；模型：{}。运行 xjcode 开始编程。",
        settings.workspace_name, settings.model
    );
    Ok(())
}
async fn doctor(home: &Path, options: xjcode_native::doctor::Options) -> Result<()> {
    println!(
        "XJCode {}\n配置目录：{}",
        env!("CARGO_PKG_VERSION"),
        home.display()
    );
    for binary in ["xjcode-engine.exe", "xjcode-login.exe"] {
        println!(
            "{}：{}",
            binary,
            if std::env::current_exe()?.with_file_name(binary).is_file() {
                "存在"
            } else {
                "缺失"
            }
        );
    }
    let credentials = auth::load_at(home)?;
    let settings = setup::load(home)?;
    let selected_model = options.model_name(home, &settings)?;
    let client = TopClient::new(credentials.clone())?;
    ensure!(
        client.identity().await? == credentials.account_id
            && settings.account_id == credentials.account_id,
        "账号校验失败"
    );
    println!("原生认证：通过");
    let models = client.models(&settings.workspace_id).await?;
    let model = models
        .iter()
        .find(|m| m.name == selected_model)
        .context("诊断模型不在交晓智当前授权目录或未运行；请刷新目录并检查模型选择")?;
    println!("工作区及模型目录：通过（{}）", model.name);
    if options.probe {
        let response = client
            .complete(&settings.workspace_id, model, "Reply exactly XJCODE_OK")
            .await?;
        ensure!(
            response.trim() == "XJCODE_OK",
            "模型已完成回复，但探针内容不符"
        );
        println!("完整模型调用：通过");
    } else {
        println!("未调用模型；完整调用检查：xjcode doctor --probe");
    }
    Ok(())
}
#[cfg(windows)]
async fn browser_login() -> Result<auth::Credentials> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let nonce = uuid::Uuid::new_v4().to_string();
    let pipe = format!(r"\\.\pipe\xjcode-login-{nonce}");
    let mut server = xjcode_native::ipc::private_pipe(&pipe)?;
    let executable = std::env::current_exe()?.with_file_name("xjcode-login.exe");
    ensure!(executable.is_file(), "缺少登录组件，请安装完整发行包");
    let profile = tempfile::Builder::new().prefix("xjcode-login-").tempdir()?;
    let mut child = tokio::process::Command::new(executable)
        .args(["--pipe", &pipe, "--nonce", &nonce, "--profile"])
        .arg(profile.path())
        .creation_flags(0x08000000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    println!("请在校园登录窗口完成登录。等待最长 5 分钟；关闭窗口可取消。");
    let receive = async {
        server.connect().await?;
        let size = server.read_u32_le().await? as usize;
        ensure!(size <= 16384, "登录响应过大");
        let mut data = vec![0; size];
        server.read_exact(&mut data).await?;
        let result =
            serde_json::from_slice::<auth::LoginResult>(&data).context("登录响应格式无效")?;
        ensure!(result.nonce == nonce, "登录校验码不匹配");
        let credentials = auth::Credentials {
            account_id: result.account_id,
            access_key: result.access_key,
            secret: auth::decode_secret(&result.encrypted_secret, &result.tenant_id)?,
        };
        credentials.validate()?;
        Ok::<_, anyhow::Error>(credentials)
    };
    let result = tokio::select! {result=tokio::time::timeout(Duration::from_secs(300),receive)=>result.context("校园登录超时").and_then(|r|r),_status=child.wait()=>Err(anyhow::anyhow!("登录窗口已关闭或初始化失败；请确认 Microsoft Edge WebView2 Runtime 已安装"))};
    // Let the GUI close its WebView cleanly before removing its temporary cookies.
    if result.is_ok() {
        let _ = server.write_all(&[1]).await;
    }
    if result.is_err()
        || tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .is_err()
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let profile_path = profile.path().to_path_buf();
    if profile.close().is_err() {
        // WebView subprocesses may release their cookie files after the GUI exits.
        let mut removed = false;
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match std::fs::remove_dir_all(&profile_path) {
                Ok(()) => {
                    removed = true;
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    removed = true;
                    break;
                }
                Err(_) => {}
            }
        }
        if !removed {
            eprintln!("临时登录目录仍被 WebView2 占用，未能立即清理。");
        }
    }
    result
}
#[cfg(not(windows))]
async fn browser_login() -> Result<auth::Credentials> {
    bail!("第一版仅支持 Windows")
}
