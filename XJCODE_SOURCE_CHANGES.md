# XJCode 修复版本：源码修改清单

整理日期：2026-10-21。本文只说明 XJCode 产品源码的修改，不包含分配给模型的具体任务脚本。

比较基线：`hanhan761/XJcode` 的 `a01cee81a4207d275940cae0125c49b0eb6ce9df`，版本 0.1.3。当前对应本地修复构建 `e4bdd630eca3`。相对基线共有 **26 个源码或辅助脚本文件发生修改/新增，归纳为 O01—O23 共 23 项优化**。归档后按用户要求通过 Fork 整理为一个总 PR；本次提交不新增产品功能。

## 一、改进了什么

| 范围 | 已实现的改动 | 编号 |
|---|---|---|
| 配置与选模 | 使用独立 `.xjcode` 项目配置；启动、恢复、切换、审查与子任务模型均按交晓智授权目录校验，缺少目录时停止；诊断使用当前选模 | O01、O02、O11 |
| 请求与错误处理 | 区分 HTTP、业务、连接、响应体与流式错误；增加脱敏诊断；消息写入单独使用 60 秒预算；完整回复丢失时严格关联后只读回查；明确未建连时额外重连一次 | O03、O04、O15、O21、O22、O23 |
| 长任务与协议 | 单条工具文本预览 8 KiB、累计文本请求预算 96 KiB；无平台用量时提供本地占用估算；区分格式纠错、摘要生成和长回复小步骤恢复；执行前保留完整校验 | O05、O06、O07、O17、O18、O19 |
| Windows 执行与命令入口 | 识别真实 Python，改善后台输出；增加 PowerShell/UTF-8/执行证据提示及联网权限错误识别；稳定命令转接便于加载新版 | O08、O09、O10、O16、O20 |
| 图片输入 | 发送真实图片附件；终端与 CLI 检查能力，拒绝时保留草稿；能力证据绑定账号、工作区及部署，避免将图片路径当作图片内容 | O12、O13、O14 |

## 二、具体修改了哪些文件

以下路径均相对源码根目录；“新增”表示接收基线中没有该文件。

| 文件 | 类型 | 修改内容 |
|---|---|---|
| `xjcode-native/src/api.rs` | 修改 | 请求/响应/流式错误分类、请求时限、图片附件传输、模型能力字段、严格回复回查、未建连时的有界重连及相应测试 |
| `xjcode-native/src/protocol.rs` | 修改 | 工具输出与累计上下文预算、文本与图片提取、Windows 和网页读取提示、格式纠错、压缩摘要协议、长回复小步骤恢复及联网失败识别 |
| `xjcode-native/src/main.rs` | 修改 | 真实 Python 发现、仅对子进程设置无缓冲输出、接入 doctor 和图片能力、刷新目录保留当前选模 |
| `xjcode-native/src/setup.rs` | 修改 | 模型目录输出包含图片能力及其来源，显示支持图片的模型；配合授权目录与能力证据保持一致 |
| `xjcode-native/src/lib.rs` | 修改 | 导出新增诊断、doctor、图片和能力模块 |
| `xjcode-native/src/diagnostics.rs` | 新增 | 有限大小的脱敏诊断、错误建议、请求/响应/流进度、协议错误、回复回查与建连尝试记录 |
| `xjcode-native/src/doctor.rs` | 新增 | 读取当前默认模型，解析显式 `-m/--model`，按实时授权目录选择短调用模型 |
| `xjcode-native/src/images.rs` | 新增 | 提取、校验真实图片，限制张数/单图/总大小，保留编号并转换为平台附件 |
| `xjcode-native/src/capabilities.rs` | 新增 | 记录并校验图片能力证据的账号、工作区及部署作用域；拒绝损坏、缺项或不匹配证据 |
| `xjcode-native/examples/probe_vision_models.rs` | 新增 | 对授权模型运行合成图片探针，记录能力验证结果；不会扩展账号的模型授权 |
| `codex-rs/config/src/loader/mod.rs` | 修改 | XJCode 模式只发现自己的项目配置目录，隔离其他工具配置 |
| `codex-rs/core/src/config/mod.rs` | 修改 | XJCode 模型配置与目录校验，阻止其他后端或内置模型替代授权目录 |
| `codex-rs/core/src/client.rs` | 修改 | 将引擎明确的 Compaction 请求标记传到原生适配，区分摘要和执行 |
| `codex-rs/core/src/session/mod.rs` | 修改 | 启动/恢复等路径的模型校验，缺少平台 usage 时更新本地上下文估算 |
| `codex-rs/core/src/session/step_settings.rs` | 修改 | 会话设置变更时检查交晓智授权模型 |
| `codex-rs/core/src/xjtu.rs` | 修改 | 适配图片和摘要阶段；协议恢复后仍校验工具参数、补丁语法及最终状态；衔接上下文估算 |
| `codex-rs/model-provider/src/provider.rs` | 修改 | 禁止 XJCode 走其他提供方的自动模型回退 |
| `codex-rs/exec/src/lib.rs` | 修改 | 命令行图片输入在创建任务前检查当前模型能力 |
| `codex-rs/tui/src/app/backend_banner_fallback.rs` | 修改 | XJCode 不沿用其他后端的自动回退行为 |
| `codex-rs/tui/src/app/config_persistence.rs` | 修改 | 保存模型选择前校验当前授权目录 |
| `codex-rs/tui/src/app/event_dispatch.rs` | 修改 | 界面切换、恢复等事件使用交晓智授权模型检查 |
| `codex-rs/tui/src/chatwidget/interaction.rs` | 修改 | 不支持图片时明确提示并保留草稿，避免静默当成文字提交 |
| `codex-rs/tui/src/chatwidget/settings.rs` | 修改 | 当前授权模型的图片支持与路径处理设置 |
| `codex-rs/tui/src/chatwidget/tests/composer_submission.rs` | 修改 | 覆盖不支持图片时保留输入及支持图片时识别路径的回归 |
| `scripts/repair-xjcode-command.ps1` | 新增 | PowerShell/cmd 专用转接到稳定 XJCode 入口，替换前备份，不修改其他工具命令 |
| `scripts/create-xjcode-vision-fixtures.py` | 新增 | 创建用于图片能力验证的合成随机码图片，避免使用个人图片 |

逐项背景、验证证据及限制见 `XJCODE_OPTIMIZATION_LOG.md`。源码内容摘要与基线 Git blob 见 `docs/xjcode/changes-from-a01cee8.json`。

## 三、验证情况

| 项目 | 已有结果 | 范围 |
|---|---|---|
| 原生模块回归 | O23 构建时 72 项通过 | 包含请求诊断、协议、图片、预算、时限、回查及建连恢复 |
| 核心 XJTU 定向回归 | O22 时 7 项通过 | 摘要/执行隔离、工具恢复及原有校验；O23 未修改核心源码 |
| 当前本地构建 | 完整引擎及启动器编译、安装摘要核验通过 | 引擎标识 `e4bdd630eca3`，不代表公开下载制品已更新 |
| 实际短调用 | O23 安装后的 `doctor --probe -m qwen3.8-max` 通过 | 认证、授权目录及该次完整模型调用 |
| 图片专项 | 24 个授权模型逐一检查，7 个双图通过 | 对应当时账号/工作区/部署，详见 `docs/xjcode/vision-20261019/REPORT.md` |
| 本次打包前核对 | 26 个源码文件与此前通过构建时的 SHA256 一致 | 本轮只更新文档，不把历史回归写成重新执行 |

## 四、交接边界

- 只允许使用交晓智平台授权模型，不新增 OpenAI/Codex 模型后端回退。
- 已发送或结果不明的请求不自动重放；重连只针对库明确标记的建连失败。只读回查必须匹配原请求，不能绕过工具校验。
- Windows 命令写法、执行证据和结果真实性提示属于行为引导，不能保证模型永不生成错误代码；客户端字节预算也不是平台公布的模型 token 上限。
- 校园登录、DPAPI、HMAC、原有安装器与基础协议已存在于基线，不重复计算为本轮新增功能。
- 源码包不包含个人配置、凭据、真实会话、构建缓存、已安装二进制或具体任务数据；使用方法见 `XJCODE_SOURCE_HANDOFF.md`。
