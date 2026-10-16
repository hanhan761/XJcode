# 网站分发维护

公开入口：https://www.wenshanzhike.com/xjcode/

本地运行 scripts/build-xjcode-distribution.mjs，产出 dist/web。服务器仅解压校验静态文件，不编译源码、不运行 npm 安装。安装时的 npm tarball、Node ZIP、EXE 和 manifest 均来自此站点。

发布到共享 Caddy 静态挂载中的 xjcode-releases/<release-id>，用相对符号链接 xjcode 指向已校验目录。切换前取得现有部署锁；切换后验证 HTTPS 首页、全部文件摘要、安装与既有网站健康，观察至少 60 秒。保留旧目录；回滚只原子切回旧链接，首次发布失败则撤下新链接。主站后续更新必须保留 xjcode 与 xjcode-releases，避免全目录覆盖。

不要上传 .xjcode、.codex、.env、PEM、个人配置或构建日志。GitHub 保存源码和发行备份；普通用户安装无需 GitHub 连通。

共享主站使用 SPA 回退，因此 /xjcode/* 需要独立静态 handler：root /srv/app，try_files {path} {path}/index.html，file_server；/xjcode 重定向到 /xjcode/。该 handler 与版本链接应一起保留。

版本化 /xjcode/releases/* 文件使用 Cache-Control: public, max-age=31536000, immutable，避免 npm 读取包清单时重复下载。已发布版本内容不可覆盖，更新时使用新版本目录。安装页与根 install.ps1 不使用长期缓存。
