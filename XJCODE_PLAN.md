# XJCode implementation

Upstream: openai/codex e790ce271749ce431e446a9e2cc66485fe60f931.

- [x] Import pinned upstream and configure reproducible Windows build.
- [x] XJTU HMAC, discovery, bounded SSE completion and structured tool protocol.
- [x] DPAPI vault and isolated WebView2 campus login with same-account key reuse (offline verified).
- [x] Native Codex transport, model catalogue, CLI and independent home.
- [x] Windows development package, installer, checksums, documentation and targeted tests.
- [x] Real campus login and coding smoke test (DeepSeek-V3-2; actual patch and PowerShell test passed).
- [x] Public self-hosted website, PowerShell bootstrap and npm tarball distribution; GitHub source publication.
- [ ] Optimized release build and broader campus pilot validation.

Evidence and remaining acceptance checks: [XJCODE_VALIDATION.md](XJCODE_VALIDATION.md).
No shared proxy service, no OpenAI model fallback, no credentials in model context/logs.

## 0.1.11 follow-through

Offline formula input, session/CLI model consistency, login-list robustness and health-summary corrections are implemented. Verification and limits: [0.1.11 acceptance](docs/xjcode/0.1.11/VALIDATION.md).

## 0.1.12 personal workspace login

Fixed valid empty team directories hiding personal spaces; separately verified identity and personal model grants for the user-authorized reproduction account. [Acceptance](docs/xjcode/0.1.12/VALIDATION.md).
