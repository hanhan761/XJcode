# XJCode 修复源码交接说明

整理日期：2026-10-21。交接对象为完整源码与优化文档，不是安装器，也没有替换网站当前下载制品。

## 阅读顺序

1. `XJCODE_SOURCE_CHANGES.md`：按 26 个源码文件列出修改位置、用途和验证记录。
2. `XJCODE_REPAIR_REPORT_V1.md`：23 项修复的整体说明、验证范围和限制。
3. `XJCODE_OPTIMIZATION_LOG.md`：O01—O23 与源码位置、验证依据的对应关系。
4. `XJCODE_VALIDATION.md`：各轮真实验证记录，按日期区分历史与本轮结果。
5. `docs/xjcode/changes-from-a01cee8.json`：相对接收基线的改动文件、原始 Git blob 和当前 SHA256。
6. `README-XJCODE.md`、`INSTALLATION.md`：项目结构、依赖、构建和安装说明。

## 源码和构建身份

- 接收 GitHub 项目：`https://github.com/hanhan761/XJcode`。
- 比较基线：`a01cee81a4207d275940cae0125c49b0eb6ce9df`；产品版本仍为 0.1.3，本包是本地修复源码快照。
- 上游引擎固定 commit：`e790ce271749ce431e446a9e2cc66485fe60f931`。
- 当前已安装的本地修复构建标识：`e4bdd630eca3`；对应引擎 SHA256：`e4bdd630eca377ea93cbff2e5e700519f9c6165b07d438cefdde1e4ce66a388d`。
- 启动器 SHA256：`21b559e24716cb2d8114d5af1a79a2af444b95955e50811c40fb34bce06e934d`。
- 构建摘要标识此前实际安装的二进制；在另一台机器重新编译，不承诺产生相同二进制摘要。本源码包不包含该二进制。
- 保留 Apache-2.0 许可、NOTICE 和上游源码文件。包中没有 `.git` 历史；基线 commit 是比较标识，不表示修复已提交或已推送。

## 构建

在 Windows x64 安装 Rust MSVC 1.95.0、Visual Studio 2022 C++ 工作负载、Windows SDK、Git，以及 Node.js 22 或以上。解压后进入 `XJCode-source`，执行：

```powershell
.\scripts\build-xjcode.ps1 -Profile dev
```

需要优化发行构建时使用 `-Profile release`。脚本及详细要求见 README；首次构建需下载锁文件指定的依赖。本包没有预装依赖、工具链或个人代理配置，校园模型调用仍需用户自己的校园授权和网络。

## 校验与上传

- ZIP 同目录的 `.sha256` 用于核验归档文件；例如 `Get-FileHash -LiteralPath <ZIP完整路径> -Algorithm SHA256`。
- 包外层 `PACKAGE_MANIFEST.json` 列出 `XJCode-source` 内每份文件的路径、字节数和 SHA256；变更清单另说明相对接收基线的差异。
- 源码归档整理后，通过 `nooiceee/XJcode` 的修复分支向 `hanhan761/XJcode` 提出一个总 PR。归档中早先的“未提交/推送”描述对应打包时快照；评审、提交与合并状态以 GitHub 记录为准，不代表公开发行制品已更新。
- 包内不含 `.build`、编译产物、个人凭据、用户配置、图片能力个人缓存、真实会话或组合梁/单摆任务数据。
- 本次不继续处理具体测试任务，也不将任务目录或仿真交付物混入产品源码包。
