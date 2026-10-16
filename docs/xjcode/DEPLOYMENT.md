# 网站分发维护

公开入口：https://xjcode.wenshanzhike.com/ ，/guide 提供同一上手教程。旧 https://www.wenshanzhike.com/xjcode/ 安装路径继续兼容。

静态教程源码位于 site/，安装脚本位于 scripts/bootstrap-xjcode.ps1。所有编译与打包在开发机完成；服务器只校验静态制品、切换版本链接与重载 Caddy。

共享 Caddy 的新域名块分别提供教程与安装文件：教程为 /srv/app/xjcode-site/current；已发布 v0.1.0 安装包复用 /srv/app/xjcode。新域名下 /releases/* 和 /XJCode-Setup.exe 直接服务同一份经过验证的文件，不重打包或修改不可变的 v0.1.0 包。网页 install.ps1 使用新域名下载全部依赖，离线 EXE 已包含依赖。

持有 /var/lock/wenshanzhike-deploy.lock 后激活。保留旧 Caddy 配置和版本目录；失败时恢复配置并切回原 current。观察至少 60 秒，检查新旧网址 TLS、安装文件摘要与主站健康。主站发布必须保留 xjcode、xjcode-releases、xjcode-site 目录与对应 Caddy 路由。

版本化 /releases/* 使用 Cache-Control: public, max-age=31536000, immutable；根网页和 install.ps1 不长期缓存。发布新二进制时使用新版本号，不覆盖已发布包。GitHub 保存源码，普通用户安装无需访问 GitHub 或 npm 官方仓库。

禁止上传账号目录、.env、PEM、凭据、个人配置或构建日志。
