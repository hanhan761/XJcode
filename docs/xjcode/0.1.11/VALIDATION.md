# XJCode 0.1.11 verification

Date: 2026-11-12. Baseline: c7236ed / 0.1.10.

Implemented:
- Offline formula editor, 13 templates, MathML preview, LaTeX/Markdown copy, Markdown export, undo/redo and tab-scoped draft restore. The launcher command does not require authentication; `/formula` dispatches locally from the TUI. KaTeX 0.19.1 is bundled with its MIT notice; no CDN or runtime dependency fetch.
- Model lists, login status, directory refresh and re-login use the default selected in config.toml, rather than an older discovery cache.
- Ambiguous access-key lists and invalid totals/entries do not create a key.
- Website health summaries treat unrecognized states as unknown. PowerShell remains the primary install path.

Executed checks:
- Core XJTU adapter: 7 passed, 0 failed. Login navigation/private-pipe component: 6 passed, 0 failed.
- Native library: 91 passed, 0 failed. Includes regression preserving a model selected in the TUI across refresh/reconfiguration.
- Login JavaScript: 11 passed, including malformed key-list cases, permission denial and uncertain creation.
- Offline browser checks: all 13 templates; wrapper normalization; clipboard and denied-permission fallback; selection insertion; undo/redo; reload draft; Markdown export; no network requests; 1440/390/320 layouts. Website status branches and command copy passed. Re-run with `XJCODE_PLAYWRIGHT_MODULE` pointing to an installed Playwright module, then `node scripts/test-xjcode-ui.mjs <output-directory>`.
- Real ConPTY terminal: `/formula` appears in the command menu and dispatches to a local launcher fixture; no model call was made.
- Launcher formula command: works without credentials and with invalid provider state; embedded HTML matches source; unknown arguments rejected without overwriting output.
- Existing campus credentials: real identity, authorized model directory and complete DeepSeek-V3-2 probe passed. In an isolated workspace, the model created a rectangle second-moment function and tests, executed the tests successfully, and an independent Node assertion suite confirmed the results and invalid-input handling.

- Isolated npm 0.1.10 → 0.1.11 upgrade passed, including tampered-package refusal, staging version verification, retained rollback, uninstall and preservation of unrelated packages and personal data.

Limits: this does not repeat fresh interactive school SSO or prove every account can use the HTTPS session fallback. The browser/session protocol and encrypted storage have automated coverage; fresh SSO remains a human-authentication acceptance step. Shared-pool status does not certify personal accounts. A short coding task does not validate all long tasks or a whole-class pilot. Optimized release builds remain a separate milestone. No real course submissions or student data were used.
