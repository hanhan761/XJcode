//! Public names for the campus launcher; protocol and storage identifiers stay unchanged.
pub(crate) fn is_xjcode() -> bool {
    std::env::var_os("XJCODE_MODE").is_some()
}

pub(crate) fn command() -> &'static str {
    if is_xjcode() { "xjcode" } else { "codex" }
}

pub(crate) fn placeholder() -> &'static str {
    if is_xjcode() {
        "描述你的需求，让 XJCode 帮你完成…"
    } else {
        "Ask Codex to do anything"
    }
}
