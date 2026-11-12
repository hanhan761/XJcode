#![cfg_attr(windows, windows_subsystem = "windows")]
mod login_navigation;
mod login_handoff;
#[cfg(windows)]
#[derive(Debug)]
enum LoginEvent {
    Close,
    HandoffFailed,
    HandoffSlow,
    Navigate(String),
}
#[cfg(windows)]
fn main() {
    if run().is_err() {
        std::process::exit(1);
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("XJCode login requires Windows");
}
#[cfg(windows)]
fn run() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::{
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    use tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        window::WindowBuilder,
    };
    use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> anyhow::Result<String> {
        let i = args
            .iter()
            .position(|a| a == name)
            .context("Missing argument")?;
        args.get(i + 1).cloned().context("Missing value")
    };
    let pipe = arg("--pipe")?;
    let nonce = arg("--nonce")?;
    ensure!(uuid::Uuid::parse_str(&nonce).is_ok(), "Invalid nonce");
    ensure!(
        pipe == format!(r"\\.\pipe\xjcode-login-{nonce}"),
        "Invalid pipe"
    );
    let profile = PathBuf::from(arg("--profile")?);
    ensure!(profile.is_absolute(), "Invalid browser profile");
    let event_loop = EventLoopBuilder::<LoginEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let navigation_proxy = proxy.clone();
    let window = WindowBuilder::new()
        .with_title("XJCode — 西安交通大学统一身份认证")
        .with_inner_size(tao::dpi::LogicalSize::new(1080.0, 760.0))
        .build(&event_loop)?;
    let mut context = WebContext::new(Some(profile));
    let script =
        include_str!("login.js").replace("__XJCODE_NONCE__", &serde_json::to_string(&nonce)?);
    let sent = Arc::new(AtomicBool::new(false));
    let handoff_failed = Arc::new(AtomicBool::new(false));
    let ipc_failed = handoff_failed.clone();
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        // Campus SSO must use the same direct route as the native API client.
        // Scope this to our isolated WebView; never modify the system proxy.
        .with_additional_browser_args("--no-proxy-server")
        .with_url("https://ai.xjtu.edu.cn/login")
        .with_initialization_script(script)
        .with_devtools(false)
        .with_navigation_handler(move |url| match login_navigation::target(&url) {
            Ok(None) => true,
            Ok(Some(secure_url)) => {
                let _ = navigation_proxy.send_event(LoginEvent::Navigate(secure_url));
                false
            }
            Err(()) => false,
        })
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_ipc_handler(move |request| {
            if request.uri().scheme_str() != Some("https")
                || request.uri().host() != Some("ai.xjtu.edu.cn")
                || request.body().len() > 16384
            {
                return;
            }
            let Ok(value) =
                serde_json::from_str::<xjcode_native::auth::LoginResult>(request.body())
            else {
                return;
            };
            if value.nonce != nonce
                || value.account_id.is_empty()
                || sent.swap(true, Ordering::SeqCst)
            {
                return;
            }
            // Pipe I/O must not block the WebView UI thread. Keep the window
            // open until the launcher explicitly acknowledges receipt.
            let bytes = zeroize::Zeroizing::new(request.body().as_bytes().to_vec());
            let pipe = pipe.clone();
            let result_proxy = proxy.clone();
            let failed = ipc_failed.clone();
            let done = Arc::new(AtomicBool::new(false));
            let slow_done = done.clone();
            let slow_proxy = proxy.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(8));
                if !slow_done.load(Ordering::SeqCst) {
                    let _ = slow_proxy.send_event(LoginEvent::HandoffSlow);
                }
            });
            std::thread::spawn(move || {
                let result = std::fs::OpenOptions::new().read(true).write(true).open(&pipe)
                    .and_then(|mut file| login_handoff::send(&mut file, &bytes));
                done.store(true, Ordering::SeqCst);
                if result.is_ok() {
                    let _ = result_proxy.send_event(LoginEvent::Close);
                } else {
                    failed.store(true, Ordering::SeqCst);
                    let _ = result_proxy.send_event(LoginEvent::HandoffFailed);
                }
            });
        })
        .build(&window)?;
    // Explicitly show after WebView creation, including when the CLI was launched hidden.
    window.set_visible(true);
    window.set_focus();
    window.request_user_attention(Some(tao::window::UserAttentionType::Informational));
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = if handoff_failed.load(Ordering::SeqCst) { ControlFlow::ExitWithCode(2) } else { ControlFlow::Exit },
            Event::UserEvent(LoginEvent::HandoffFailed) => {
                window.set_title("XJCode — 登录交接失败，请返回 PowerShell 重试");
                let _ = webview.evaluate_script("window.dispatchEvent(new Event(\"xjcode-handoff-failed\"))");
            }
            Event::UserEvent(LoginEvent::HandoffSlow) if !handoff_failed.load(Ordering::SeqCst) => {
                let _ = webview.evaluate_script("window.dispatchEvent(new Event(\"xjcode-handoff-slow\"))");
            }
            Event::UserEvent(LoginEvent::Close) => *control_flow = ControlFlow::Exit,
            Event::UserEvent(LoginEvent::Navigate(url)) => {
                if webview.load_url(&url).is_err() {
                    window.set_title("XJCode — 校园认证页面打开失败，请重新登录");
                }
            }
            _ => {}
        }
    });
}
