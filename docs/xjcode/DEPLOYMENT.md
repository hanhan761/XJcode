# 网站分发维护

公开入口：https://www.wenshanzhike.com/xjcode/

本地运行 scripts/build-xjcode-distribution.mjs，产出 dist/web。服务器仅解压校验静态文件，不编译源码、不运行 npm 安装。安装时的 npm tarball、Node ZIP、EXE 和 manifest 均来自此站点。

发布到共享 Caddy 静态挂载中的 xjcode-releases/<release-id>，用相对符号链接 xjcode 指向已校验目录。切换前取得现有部署锁；切换后验证 HTTPS 首页、全部文件摘要、安装与既有网站健康，观察至少 60 秒。保留旧目录；回滚只原子切回旧链接，首次发布失败则撤下新链接。主站后续更新必须保留 xjcode 与 xjcode-releases，避免全目录覆盖。

不要上传 .xjcode、.codex、.env、PEM、个人配置或构建日志。GitHub 保存源码和发行备份；普通用户安装无需 GitHub 连通。
