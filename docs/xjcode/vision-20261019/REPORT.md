# 交晓智 24 个授权模型图片输入实测与适配记录

日期：2026-10-19，北京时间。接口矩阵开始于 2026-10-19 10:06:44，含文字对照的记录更新至 2026-10-19 10:11:09。

## 已验证的结论

- 当前账号的 24 个授权模型均发起了图片检查；7 个通过两张独立随机码图片，5 个完整回复但未识别，12 个图片请求出现接口错误。
- 12 个错误模型追加不同输入的纯文字对照：10 个文字也失败；qwen-long 和 qwen3.7-max 文字回复正确，但带图异常。错误模型的视觉能力仍未确认。
- 7 个双图通过模型均已通过完整 XJCode 客户端的第三张新图检查，退出码为 0，未执行工具。
- 相比仅依赖平台 vision 字段，新增确认了 6 个可收图的授权模型。qwen3-max 仍只是平台声明支持，本轮图片和文字均 HTTP 500，未列入实测成功。

这些结果描述当前平台部署的图片输入表现，不证明底层模型架构，也不是复杂编程、Abaqus 或工程精度测试。

## 全部结果

| 模型 | 平台声明图片 | 图片实测 | 文字对照 | 完整客户端 | 验证时入口标记 |
|---|---|---|---|---|---|
| `DeepSeek-V3-2` | 未标记 | 未识别/称未收到 | — | — | 未启用额外图片能力 |
| `Doubao-Seed-2.0-pro` | 未标记 | 双图通过 | — | 通过，42.94s | 图片·实测 |
| `Doubao-pro-32k` | 未标记 | 双图通过 | — | 通过，3.85s | 图片·实测 |
| `GLM-4-Air` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `GLM-4-Long` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `GLM-4-Plus` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `GLM-4.7` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `GLM-5.2` | 未标记 | 未识别/称未收到 | — | — | 未启用额外图片能力 |
| `doubao-seed-2.0-lite` | 未标记 | 双图通过 | — | 通过，5.15s | 图片·实测 |
| `moonshot-v1-128k` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `moonshot-v1-32k` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `moonshot-v1-8k` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `qwen-long` | 未标记 | HTTP 200 / stream_empty | 正确回复 | — | 未启用额外图片能力 |
| `qwen-max` | 未标记 | 未识别/称未收到 | — | — | 未启用额外图片能力 |
| `qwen-plus` | 未标记 | 未识别/称未收到 | — | — | 未启用额外图片能力 |
| `qwen-plus-latest` | 未标记 | 未识别/称未收到 | — | — | 未启用额外图片能力 |
| `qwen-turbo` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `qwen3-coder-plus` | 未标记 | HTTP 500 / http | HTTP 500 | — | 未启用额外图片能力 |
| `qwen3-max` | 是 | HTTP 500 / http | HTTP 500 | — | 图片·平台声明 |
| `qwen3-vl-plus` | 是 | 双图通过 | — | 通过，4.32s | 图片·实测 |
| `qwen3.6-flash` | 未标记 | 双图通过 | — | 通过，7.78s | 图片·实测 |
| `qwen3.7-max` | 未标记 | HTTP 500 / http | 正确回复 | — | 未启用额外图片能力 |
| `qwen3.7-plus` | 未标记 | 双图通过 | — | 通过，2.62s | 图片·实测 |
| `qwen3.8-max` | 未标记 | 双图通过 | — | 通过，3.83s | 图片·实测 |

## 测试方法与边界

1. 在内存中解密现有登录凭据，通过学校 TOP 客户端读取实时授权列表；不测试未授权模型。探测时只对已授权 Model 的临时副本开放图片发送，不提前改生产目录。
2. 使用新生成的两张 800×440 PNG，每张含不同的 8 位随机码和两种图形。验证码只存在于图片像素，未放进提示词、文件名或工具结果。首张未识别或接口失败时不继续第二张；两张都正确才记为实测支持。
3. 图片通过 ExtendsInfo.Files 发送，保留 HMAC、SSE 完整回复验证。每次新建独立平台会话，不在错误后自动重试同一请求；文字对照是独立的新输入。
4. 将双图结果绑定账号、工作区及部署身份写入本机能力记录，再使用完整客户端、独立只读目录和第三张新图验证 7 个通过模型。没有在合成图中使用个人信息，也未操作 Abaqus。
5. 50 项原生库回归通过，包含跨账号/工作区/部署拒绝、无效证据不启用、目录刷新保留能力；终端的两项交互回归在前一轮通过。本轮未自动操控系统剪贴板。

## 测试素材与脱敏材料

- [JSON 明细](results.json)：含回复、耗时、图片摘要和完整客户端结果。
- [CSV 汇总](results.csv)：适合表格软件筛选。
- [第一张图片](fixture-1.png)、[第二张图片](fixture-2.png)、[客户端第三张图片](fixture-3.png)：均为合成素材。
- 导出材料不包含账号/工作区/服务 ID、密钥、原始诊断路径或真实会话。原始本地证据单独保存，不随源码上传。

## 复现与重新验证

使用含本修复的源码与构建环境，先按 README 完成校园登录。生成新素材需要 Python 和 Pillow >= 10。每次选择一个新的测试目录，避免覆盖旧证据。

```powershell
python scripts/create-xjcode-vision-fixtures.py F:/XJCodeModelTests/vision-new
cargo run --locked --manifest-path xjcode-native/Cargo.toml --example probe_vision_models -- F:/XJCodeModelTests/vision-new
cargo run --locked --manifest-path xjcode-native/Cargo.toml --example probe_vision_models -- F:/XJCodeModelTests/vision-new --text-controls
# 只有此显式步骤才将双图通过结果写入本机能力记录
cargo run --locked --manifest-path xjcode-native/Cargo.toml --example probe_vision_models -- F:/XJCodeModelTests/vision-new --apply-results
xjcode models --refresh
```

平台声明与实测结果独立保存；刷新目录按账号及部署重新匹配，不复制他人的本机能力文件。平台更换部署标识会使旧实测不适用；若平台保留标识但改变服务，应重新测量。

## 构建和当前默认值

界面文案随后按用户要求简化：已确认模型仅显示“支持图片”，命令列表显示 `[图片]`；下列摘要对应实际执行识图验收的构建，测试结果与能力来源保留。

- 完整客户端验收引擎 SHA256：`8a5a8ae7d66e8491d973f2da504258f4ae5d7f314452eb72adc56dac6bea5e45`。
- 启动器 SHA256：`b040d5377b9e25b4e72ecf0a8864b6d09ad6fcc79518838f74760610aad3d978`。
- 默认模型仍为 qwen3.7-max；图片测试均显式指定模型，没有自动切换默认值。已有旧窗口需退出后重新打开。
- 本轮仅更新本机及源码工作区，没有上传或发布。相对原版的全部优化见 [优化记录](../../../XJCODE_OPTIMIZATION_LOG.md)。
