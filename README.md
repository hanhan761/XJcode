# XJCode

面向 Windows PowerShell 的交晓智编程工具。使用校园账号登录，在项目里理解代码、修改文件并运行测试。

[官方网站与一键安装](https://www.wenshanzhike.com/xjcode/) · [使用文档](README-XJCODE.md) · [安装说明](INSTALLATION.md)

## 安装

PowerShell 一条命令，自动准备缺少的 Node.js：

```powershell
irm https://www.wenshanzhike.com/xjcode/install.ps1 | iex
```

已有 Node.js 22+：

```powershell
npm install -g https://www.wenshanzhike.com/xjcode/releases/0.1.0/xjcode-0.1.0.tgz
```

也可以从网站下载单文件安装器，双击安装。下载由国内自有服务器提供，无需通过 GitHub 或 npm 官方仓库。支持 Windows x64，校园登录需要 WebView2 和有效校园账号。

```powershell
xjcode login
cd D:\your-project
xjcode
```

使用独立的 xjcode 命令和 .xjcode 配置目录，可以与 Codex 同时安装。

## 开源说明

基于 OpenAI Codex commit `e790ce271749ce431e446a9e2cc66485fe60f931` 开发，保留 [Apache-2.0 LICENSE](LICENSE) 和 [NOTICE](NOTICE)。本项目不是 OpenAI 或学校官方发行版。构建与模型数据处理说明见[使用文档](README-XJCODE.md)。
