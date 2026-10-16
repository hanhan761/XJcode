# XJCode（Windows 校园试用版）

XJCode 是基于 OpenAI Codex 源码的本地编程命令行工具，使用西安交通大学交晓智作为模型后端。上游固定在 commit `e790ce271749ce431e446a9e2cc66485fe60f931`；保留 Apache-2.0 许可证及 NOTICE。本项目不是 OpenAI 或学校的官方发行版。

## 安装与开始

普通用户：从[官方网站](https://www.wenshanzhike.com/xjcode/)下载并双击 **XJCode-Setup.exe**，自动准备 Node.js 和 XJCode，无需解压或管理员权限。安装完成后重新打开 PowerShell。

已有 Node.js 22 或以上：直接用 npm 安装 .tgz 文件或网站提供的 tarball URL。安装命令：

~~~powershell
npm install -g https://www.wenshanzhike.com/xjcode/releases/0.1.0/xjcode-0.1.0.tgz
xjcode login
cd D:\your-project
xjcode
~~~

网站安装器和 npm 包均自带原生组件；npm 包无需 postinstall 脚本和二次下载。网站提供直接安装 URL，npm 官方短包名尚未发布，详见 [安装与发布说明](INSTALLATION.md)。Windows x64 和 WebView2 Runtime 为运行要求。

`xjcode login` 打开独立校园登录窗口。完成学校认证后，程序自动读取当前账号的有效 API 密钥、核验账号归属，列出工作区及授权模型供选择。不会要求手工复制 token，也不会读取其他工具保存的凭据。已登录且密钥有效时，再次 login 直接复用；`xjcode login --force` 重新打开校园认证，可切换账号。

```powershell
xjcode login status
xjcode models
xjcode models --refresh
xjcode models --select
xjcode doctor --probe
xjcode exec "检查当前项目的测试失败原因并修复"
xjcode resume
xjcode mcp --help
xjcode logout
```

`doctor --probe` 会调用一次当前模型；普通 `doctor` 只核验身份和模型目录。读写文件、执行命令、审批与沙箱、会话恢复、AGENTS.md、skills 和本地 MCP 由 Codex 引擎提供。具体 CLI 参数见 `xjcode exec --help`。交互界面部分提示仍沿用上游英文。

## 与本机 Codex 并存

| 项目 | XJCode | 已有 Codex |
| --- | --- | --- |
| 命令 | `xjcode` | `codex` 保持原样 |
| 安装目录 | 网站安装器使用 `%LOCALAPPDATA%\XJCode`；npm 使用独立 `xjcode` 包目录 | 保留原位置 |
| 凭据、配置和会话 | `%USERPROFILE%\.xjcode` | `.codex` 保持原样 |
| 数据库 | XJCode 子进程独立指定 `.xjcode` | 不继承到 XJCode |

安装器只添加 XJCode 自己的 PATH 项，不设置全局 `CODEX_HOME`，不修改 PowerShell profile、不创建 `codex` 别名。引擎环境覆盖仅对 XJCode 子进程生效。网站安装器拒绝安装到 `.codex` 目录。`AGENTS.md` 仍是项目本身的协作规则。

## 登录和数据

- 校园密码只输入学校原始登录页面；程序不采集密码。登录组件只接受校园域名的 HTTPS 导航；学校两个已确认的历史 HTTP 跳转会自动升级为 HTTPS。
- 使用独立临时 WebView2 数据目录。登录窗口和原生 API 都直连校园服务，避免系统代理导致 `ERR_CONNECTION_CLOSED`；不修改系统代理。
- 优先复用当前账号最新的有效 API 密钥。没有有效密钥且不足两个时创建一个；已有两个但均不可用时提示用户在校园平台管理，不擅自删除或禁用任何密钥。
- API secret 解密后以 Windows DPAPI 当前用户方式加密保存于 `%USERPROFILE%\.xjcode\credentials.dpapi`。用户可通过绝对路径 `XJCODE_HOME` 改变目录。配置及会话与 `.codex` 分离。
- 登录组件通过限当前用户访问的命名管道交付凭据，不通过命令参数、日志或模型上下文传递密钥。logout 仅删除本机凭据，不撤销服务器密钥，也不删除已有会话。
- 模型请求直接调用学校原生 HMAC TOP 接口，无本地 HTTP 中转服务，无 OpenAI 模型回退。当前平台原生入口是 `http://ai.xjtu.edu.cn:30040/`；HMAC 验证请求签名，**不提供传输加密**。代码上下文和工具结果会发送到校园平台，并形成平台会话，应在受信校园网络使用。网页认证仍使用 HTTPS。

## 第一版范围

当前适配文本模型。默认保守上下文窗口为 32768 tokens；可通过引擎配置调整，但需与实际模型容量匹配。模型目录来自当前账号的真实授权，不把未验证能力标为可用。图像、语音、OpenAI Web Search、云任务、远程 app-server 及自动更新不在本版范围。

交晓智 Chat 返回文本，因此适配层将工具定义与历史转为显式 JSON 协议。只在完整响应结束并通过工具名称、参数 schema 校验后，交给 Codex 执行。格式错误最多修复两次；网络不确定或限流不会自动重发 Chat。模型格式遵循能力影响实际编程效果。本版先收完整回复再展示，不逐字显示尚未确认完成的回复。

## 从源码构建

安装 Rust MSVC toolchain 1.95.0、Git、Node.js 22 或更新版本（运行登录脚本测试）、Visual Studio 2022 C++ 工作负载和 Windows SDK。在仓库根目录执行：

```powershell
.\scripts\build-xjcode.ps1 -Profile dev
# 优化发行包（首次编译较久）
.\scripts\build-xjcode.ps1 -Profile release
```

脚本识别 Visual Studio，使用锁文件构建引擎、登录器和 Windows 沙箱辅助程序，运行交晓智针对性测试，生成 `dist\xjcode-0.1.0-windows-x64-<profile>.zip`。包内附 SHA256 校验清单，安装前核验。源码下载如需代理，请仅在本机配置 Cargo/Git；不要把代理凭据提交到仓库。

模块职责：

| 路径 | 职责 |
| --- | --- |
| xjcode-native/src/main.rs | xjcode 命令、登录编排和引擎启动 |
| xjcode-native/src/login.rs、login.js | 独立校园 WebView2、账号密钥获取 |
| xjcode-native/src/auth.rs、ipc.rs | DPAPI 凭据和受限命名管道 |
| xjcode-native/src/api.rs | HMAC 签名、账号/模型发现、SSE 完整性 |
| xjcode-native/src/protocol.rs | 模型工具协议及 schema 验证 |
| xjcode-native/src/setup.rs | 独立配置和 Codex 模型目录 |
| codex-rs/core/src/xjtu.rs | 原生 provider 到 Codex 事件的映射 |
| scripts/build-xjcode.ps1、install-xjcode.ps1 | 构建、打包、用户级安装 |

## 故障排查

- 登录窗口无法连接：确认校园网络可达。当前窗口强制直连；依赖代理或 VPN 的校外环境需先建立学校提供的网络连接。
- 白屏或点击登录无反应：安装最新包，已修复校园认证旧式 HTTP 跳转被拦截的问题。
- 登录窗口初始化失败：检查 WebView2 Runtime 是否已安装。
- 登录后出现密钥上限：到交晓智个人设置检查自己的密钥状态，然后重新 login；程序不会替你撤销密钥。
- 401：重新 login；403：检查工作区/模型权限；429：等待后重试。
- Chat 返回 500：该次上游调用失败，可用 `xjcode models --select` 切换授权模型，再运行 `xjcode doctor --probe`；目录中有模型不代表实际请求一定成功。
- 回复被截断或工具格式连续不合法：不会执行该轮工具；减少上下文或换用更适合编程的授权模型。
- 模型目录变化：运行 `xjcode models --refresh`；默认模型已下线时运行 `xjcode login` 重新选择。
