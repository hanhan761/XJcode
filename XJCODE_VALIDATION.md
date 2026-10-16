# XJCode 验证记录

验证环境：Windows x64；Rust 1.95.0 MSVC；Visual Studio 2022 C++ 14.44；Windows SDK 10.0.26100.0。上游 commit：e790ce271749ce431e446a9e2cc66485fe60f931。

## 已通过

- 原生库 11 项测试：HMAC 签名与交晓智 skill 的 Python 实现一致；SSE 截断/错误拒绝；存储回复校验；AES-CFB 解密；日志脱敏；DPAPI 往返及篡改拒绝；当前用户命名管道帧与确认；工具参数 schema；保留用户扩展配置；损坏配置不覆盖原有账号。
- 登录脚本 5 项测试：复用最新有效密钥；容量允许才创建；两个不可用密钥不创建、不删除、不启用；创建结果不明时不重复；非平台 origin 不访问 IAM 或回传凭据。
- Codex core 2 项针对性测试：真实 ModelInfo 目录反序列化；完整回复事件顺序。未运行全部上游测试。
- 编译后的引擎使用隔离测试目录启动成功。无凭据时输出“尚未登录，请运行 xjcode login”，没有进入 OpenAI 登录流程。
- 全局参数在 login 前面的调用被引擎拦截，避免绕过校园登录入口。
- Windows unelevated 沙箱实际执行隐藏 PowerShell，成功在测试工作目录写入并输出 XJCODE_SANDBOX_OK。
- git diff --check 通过；PowerShell 构建/安装脚本语法解析通过。

## 本次登录错误

原始截图：ai.xjtu.edu.cn 的 ERR_CONNECTION_CLOSED。

同一机器、同一 URL 的差异验证：
- curl 直连 https://ai.xjtu.edu.cn/login：HTTP 200。
- 经过系统代理 127.0.0.1:7890：连接失败（curl exit 35，HTTP 000）。

登录 WebView2 改为专用 --no-proxy-server 参数，与原生客户端直连策略一致，不修改系统设置。另补充 set_visible / set_focus；Windows IsWindowVisible 对修复后的登录窗口返回 true。

## 2026-10-16 白屏与登录回调修复

实际 WebView2 DOM 为空，排除了仅窗口未显示的假设。校园平台使用两个 HTTP 格式的跳转：org.xjtu.edu.cn/openplatform/oauth/authorize 和 ai.xjtu.edu.cn/api/auth/callback；旧导航策略将其拦截。现仅将这两个已确认主机的无自定义端口 HTTP 导航升级为 HTTPS，保留路径和查询参数；外部站点、非网络 scheme 和带用户信息的 URL 仍拒绝。

新增 4 项导航回归测试，修复前回调用例失败，修复后全部通过。此次重新运行原生库 11 项、导航 4 项、登录脚本 5 项，共 20 项通过；原先 Codex core 2 项未重复运行。补充临时 WebView2 目录释放的有限清理重试。诊断调试端口已关闭，诊断浏览器目录已删除，调试参数未写入发行代码。

## 已完成的在线验收

- 使用用户授权的校园账号走通真实认证：原始校园页面登录、平台回调、同账号密钥自动获取、原生身份核验、授权目录读取及 DPAPI 凭据保存。
- qwen3-coder-plus：一次真实调用在 Chat 阶段返回 HTTP 500；未自动重放，不能据此判定永久不可用。
- 切换至 DeepSeek-V3-2：xjcode doctor --probe 完整回复校验通过，当前默认模型为该模型。
- 已安装 xjcode exec 在独立 .build/coding-smoke 项目真实调用读文件、apply_patch 和 PowerShell：将 sum.ps1 的减法改为加法，test.ps1 原文保持不变，测试输出 XJCODE_CODING_SMOKE_OK，进程退出 0。测试进程最初因诊断环境 PATH 缺少 PowerShell 目录失败，智能体改用完整路径后通过。
- git diff --check 通过。

尚未验证换账号、服务端密钥失效和真实校园限流；未进行全模型可用性扫描。当前为开发试用版，优化 release 构建与更广泛校园试用仍待后续验证。

## 发行包检查

开发试用包已编译、压缩；ZIP 内 11 个文件 CRC 校验通过。安装脚本已在隔离目录完成 SHA256 全文件校验、复制、版本检查；xjcode --version 与 xjcode exec --help 均成功。首次压缩曾遇到文件临时占用，重试成功；构建脚本已增加有限重试。

已安装到 %LOCALAPPDATA%\XJCode\bin，用户 PATH 已更新；从刷新用户 PATH 的 PowerShell 中直接运行 xjcode --version 成功。安装器使用 Windows 已知文件夹 API，兼容缺失 LOCALAPPDATA 环境变量的启动环境。

2026-10-16 修复包已重新生成，ZIP 11 文件 CRC 通过；已安装文件 SHA256 与发行包清单逐项一致。安装器在补齐诊断进程 PATHEXT 的环境中通过版本检查；未修改系统环境设置。真实编程子进程退出码为 0。

## 终端界面与安装隔离更新

XJCode 独立青绿色主题、中文欢迎区、模型/项目路径和中文输入提示已完成，使用 ConPTY 实际启动编译后的引擎并捕获画面验证。预览图 docs/xjcode/terminal-preview.png 由实际捕获的终端字符及颜色渲染，不是设计稿。

已增加双击 Install-XJCode.cmd 入口；安装器拒绝 .codex 目录，负例验证通过。引擎子进程明确隔离 CODEX_SQLITE_HOME。安装前后本机 Codex config.toml、auth.json 内容 SHA256 一致；codex 命令仍指向原 npm 安装，xjcode 指向独立目录。新版安装成功、doctor 身份与目录核验通过、ZIP 12 文件 CRC 通过。未再次调用模型（本次仅界面及安装修改）。

公开分发使用国内云服务器的独立静态目录；具体入口见 INSTALLATION.md。

## npm 与无 Node.js 一键安装验收（2026-10-16）

- 最终 npm 包直接包含原生程序，无生命周期安装脚本。npm install -g 本地 TGZ --offline --ignore-scripts 成功，安装后 xjcode --version 返回 0。
- 本机临时 HTTP 下载地址安装 npm tarball 成功，安装后版本检查通过，npm uninstall 清理该测试 prefix 成功。此项验证下载地址安装机制，不代表公开 HTTPS 域名已经上线。
- XJCode-Setup.exe 在 PATH 中完全移除 Node.js 的测试环境中成功：安装私有官方 Node.js v24.22.0、npm 和 XJCode，版本检查分别通过。无需管理员权限；测试使用独立目录，不改测试机用户 PATH。
- 图形安装器的同一安装实现使用 --silent 验收；未将“仅编译成功”当作安装成功。
- 损坏 TGZ 校验失败时，既有命令入口 SHA256 保持不变。
- 已使用最终 EXE 更新本机用户安装；xjcode 指向 LocalAppData/XJCode/commands/xjcode.cmd，codex 仍指向原 npm/codex.ps1。原 Codex config.toml、auth.json 和用户 .npmrc 内容指纹未变。
- npm tarball 共 18 个文件，包含原生引擎，不含 PEM、DPAPI 凭据或 .env。安装器约 160 MiB，npm 包约 124 MiB。
- npm 官方短包名尚未发布，使用自有网站 tarball URL 安装。

## 公网分发上线（2026-10-16）

- 公开入口 https://www.wenshanzhike.com/xjcode/，网站、安装脚本、EXE、Node ZIP、npm tarball 均由国内自有服务器提供。关闭代理的本机 HTTPS 检查通过；/xjcode 自动重定向到 /xjcode/。
- 服务器校验发布包与全部文件 SHA256 后激活独立目录，60 秒观察通过。临时直传入口已撤销，未新增后台服务。
- 公网 PowerShell 安装脚本成功下载并校验 npm 包和官方 Node ZIP；无 Node 的隔离目录自动安装运行时。测试进程初始 PATHEXT 异常导致 npm 未启动，恢复机器标准 PATHEXT 后，同一安装实现完成 npm 安装与版本检查；未更改系统环境。
- 主站完整健康检查的前后差异为空：主站与教学入口正常，原有三个 MatterSwarm 域名证书检查仍失败，未改动这些服务。
- 源码已推送至 https://github.com/hanhan761/XJcode 的 main 分支，保留上游许可证与来源。源码扫描未发现校园账号、密码、私钥或 token。发行包不进入 Git 源码历史。
- 最终公开 EXE 在移除系统 Node.js 的隔离环境中再次安装成功，使用私有 Node.js，npm 安装及 xjcode --version 均返回 0。
- 发布目录已启用版本文件长期缓存；主站默认 no-store 曾导致 npm 重复下载，现公开响应头已验证为 public, max-age=31536000, immutable。
- 关闭代理、独立 npm 配置和缓存下，直接 npm install -g https://www.wenshanzhike.com/xjcode/releases/0.1.0/xjcode-0.1.0.tgz --ignore-scripts 成功（退出 0），安装后版本检查通过。该次测试在缓存规则修复前启动，共下载三次，耗时约 16 分钟；不能把这次耗时宣传为正常安装速度。

## 独立域名与三步教程（2026-10-16）

- https://xjcode.wenshanzhike.com/ 与 /guide 已启用受信任 HTTPS。DNS 与公网直连验证通过；首次 ACME 网络超时自动回滚，重试签发成功。
- 教程为安装、校园登录、第一个任务三个步骤；提供命令复制、折叠 npm 安装和常见问题。桌面、390px 与 320px 手机布局无横向溢出，目录高亮及复制内容验证通过，正式页面无 JavaScript 错误。
- 新域名下 EXE、npm tarball 与 Node ZIP 的 HTTPS Range 请求均返回 206，下载字节与已验证制品一致；发行 manifest 完全一致。复用现有不可变二进制包，没有重新修改 0.1.0 包。
- 新域名下载的 install.ps1 与源码 SHA256 一致，默认依赖地址全部为新域名；在隔离安装目录用本地已校验包执行，安装及版本检查返回 0。本次没有重复下载完整大型安装包。
- 最终静态教程版本为 20261016-v3，独立 current 原子切换，旧教程版本与 Caddy 配置保留；主站健康检查没有新增失败。原有三项 MatterSwarm 域名证书失败仍不在本任务变更范围。

## 0.1.1 品牌与会话恢复修复

- 西交官网校徽生成内嵌占用掩码，在校园模式下使用 Braille 点阵与旋转光照；正面保持校徽比例，静止状态提高细线对比度。
- 修复退出 UUID 恢复命令与会话名称选择提示；加载占位、子命令帮助、状态页校园账号提示、常用菜单和单次任务标题使用 XJCode。校园退出说明不再调用上游账号退出入口。
- 真实旧版本恢复/退出可复现 codex resume；新引擎对同一已保存会话通过恢复/退出检查。回归脚本 scripts/check-xjcode-branding.py 使用 ConPTY，不向模型发送任务。
- 专门退出文案测试 1 项通过；既有动画布局、淡出、暂停及颜色覆盖测试 7 项通过。exec、resume、mcp、review 的 --help 均退出 0，未检测到旧命令名称。
- 离线安装器在隔离、无 Node PATH 环境完成私有 Node 配置并返回 xjcode 0.1.1。本机已使用最终包升级，刷新真实用户 PATH 的 PowerShell 运行 xjcode --version 返回 xjcode 0.1.1 · 交晓智；已安装引擎再次通过原会话恢复/退出检查。
- 最终实际终端预览：docs/xjcode/terminal-0.1.1.png。仍在运行的旧终端需退出后重新启动才能载入新版。

- 源码修复已推送 main（01c76e94）。本次官网制品上传因管理链路持续超时且吞吐过低而未完成，未写入发布就绪标记；公开入口仍保留完整的 0.1.0，不应称为已发布 0.1.1。最终本机制品保留在 dist/web，发布归档 dist/xjcode-publish-0.1.1.zip 的 SHA256 为 13b02b6132acd4e2b739568a715dc21dfaf16097dba5163488a9ab87978e9efa。
- 站外直连探测 20/20 通过；服务器内部健康检查与上线前相同，仅三个既有 MatterSwarm 本地路由检查失败。
- 已终止本次等待激活的远程进程并释放部署锁，服务器确认包指针仍为 20261016-0.1.0、教程仍为 v3；未完成的 GitHub 发行条目已移除，源码提交保留。
