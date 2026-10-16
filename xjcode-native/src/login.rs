#![cfg_attr(windows, windows_subsystem = "windows")]
mod login_navigation;
#[cfg(windows)]
#[derive(Debug)]
enum LoginEvent {
    Close,
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
        io::{Read, Write},
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
            let result = (|| -> std::io::Result<()> {
                let mut file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&pipe)?;
                let bytes = request.body().as_bytes();
                file.write_all(&(bytes.len() as u32).to_le_bytes())?;
                file.write_all(bytes)?;
                file.flush()?;
                let mut ack = [0];
                file.read_exact(&mut ack)
            })();
            let _ = result;
            let _ = proxy.send_event(LoginEvent::Close);
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
            } => *control_flow = ControlFlow::Exit,
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
