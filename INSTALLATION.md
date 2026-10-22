# XJCode 一键安装与 npm 分发

> 当前官网版本：0.1.5。安装脚本、npm 包和离线安装器均已同步，包含 PR #12 的模型配置隔离、长任务恢复和图片输入修复。验收边界见 XJCODE_VALIDATION.md。

## 面向普通用户：单文件安装器

从[官方网站](https://xjcode.wenshanzhike.com/)下载 [XJCode-Setup.exe](https://xjcode.wenshanzhike.com/XJCode-Setup.exe) 后双击即可。无需解压、Node.js、npm 或管理员权限。安装器自动选择已有兼容 Node.js；没有兼容版本时，在 XJCode 私有目录安装已校验的官方 Node.js v24.22.0，不替换系统 Node.js。运行时、npm 包和原生组件全部包含在 EXE 中，下载完成后可以离线安装。校园登录和模型调用仍需校园服务可达；登录界面需要 Microsoft Edge WebView2 Runtime。

完成后重新打开 PowerShell：

~~~powershell
xjcode login
cd D:\your-project
xjcode
~~~

## 面向已有 Node.js 的用户：npm 一条命令

需要 Node.js 22 或以上。安装包自带原生程序，无 postinstall 脚本和二次下载：

~~~powershell
npm install -g https://xjcode.wenshanzhike.com/releases/0.1.5/xjcode-0.1.5.tgz
~~~

这是直接安装网站提供的 npm tarball，不依赖 npm 仓库发布。短命令 npm install -g xjcode 则必须先将 npm/xjcode 发布到 npm 官方仓库。本机 npm whoami 返回 ENEEDAUTH，因此尚未发布；不能宣称短命令已经可用。

## 隔离、升级和卸载

- npm 直接安装：使用 npm 当前全局 prefix，仅安装 xjcode 包及同名命令，不更改 prefix 或已有 @openai/codex 包。
- 网站 EXE：安装到 %LOCALAPPDATA%/XJCode/apps/<版本与包哈希>，专用命令位于 XJCode/commands，私有 Node 位于 XJCode/runtime。仅添加自己的用户 PATH 项，不改系统 Node.js 或用户 npm 配置。
- 没有 Node.js 时额外提供 node/npm 命令入口；已有 Node.js 时不创建替代入口。
- 配置、登录和会话仍位于 %USERPROFILE%/.xjcode；CODEX_HOME 和 CODEX_SQLITE_HOME 仅在 XJCode 子进程内指定。
- 升级网站版本时保留先前版本，验证新版本后才切换启动入口；凭据不进入安装包。
- npm 直接安装的用户可用 npm uninstall -g xjcode 卸载，校园凭据与会话保留。
- 图形安装器目前未做 Windows 代码签名；首次下载可能显示 Windows 发布者提示。

## 构建与网站发布

先构建原生包，再执行：

~~~powershell
node scripts/build-xjcode-distribution.mjs
~~~

产物：dist/xjcode-0.1.5.tgz；dist/web/XJCode-Setup.exe；dist/web/index.html；dist/web/install.ps1；dist/web/releases/0.1.5/（Node 官方 ZIP、npm 包、校验清单、原生 ZIP）。只发布 dist/web，不上传账号目录或源码工作区。Node 版本及 SHA256 固定在 scripts/node-runtime.json，来源为 https://nodejs.org/dist/v24.22.0/SHASUMS256.txt。

公开入口使用现有国内服务器上的独立域名 https://xjcode.wenshanzhike.com/ 。旧 www.wenshanzhike.com/xjcode 下载链接继续兼容。安装器、npm tarball 和 Node.js ZIP 均由同一网站提供；安装不依赖 GitHub 或 npm 官方仓库。

无需 Node.js 的 PowerShell 安装命令：

```powershell
irm https://xjcode.wenshanzhike.com/install.ps1 | iex
```

0.1.5 支持 `xjcode update [--check]` 和 `xjcode uninstall [--purge] [--yes]`。托管安装在 Windows 已安装应用中注册卸载入口；npm 安装通过原 prefix 维护。旧版先重跑官网安装命令一次。安装包、release-manifest.json 与 latest.json 必须作为同一版本原子发布。
