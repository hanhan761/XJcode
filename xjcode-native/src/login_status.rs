//! Safe descriptions of login-window termination; no credential material.
pub fn child_exit(code: Option<i32>) -> &'static str {
    match code {
        Some(0) => "登录窗口已关闭，XJCode 尚未收到完整登录信息。如果网页已登录，请重新运行 xjcode login，等待窗口自动关闭并留意窗口底部提示。",
        Some(2) => "网页登录后与 XJCode 的本机交接失败。请重新运行 xjcode login；若仍失败，检查安全软件是否阻止本机通信。",
        Some(1) => "登录窗口无法初始化或异常退出。请检查 Microsoft Edge WebView2 Runtime、校园网络和安全软件后重试。",
        _ => "登录窗口意外退出，XJCode 未完成登录。请重新运行 xjcode login；如持续发生，请检查校园网络和安全软件。",
    }
}
#[cfg(test)] mod tests {
 #[test] fn close_is_not_misdiagnosed_as_missing_runtime() {
  assert!(!super::child_exit(Some(0)).contains("WebView2"));
  assert!(super::child_exit(Some(0)).contains("自动关闭"));
  assert!(super::child_exit(Some(2)).contains("交接失败"));
  assert!(super::child_exit(Some(1)).contains("WebView2"));
 }
}
