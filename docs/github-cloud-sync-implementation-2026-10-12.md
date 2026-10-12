# GitHub 私有仓库云同步实现说明

## 用户场景

在家里编辑备忘或业务流程后，尽快上传；到公司打开 Lumen 后自动下载查看。GitHub 作为可选同步位置，不替代现有坚果云 / WebDAV。两台电脑仍使用 Lumen 的本地数据库和云同步冲突处理；GitHub 不要求桌面客户端常驻。

## 连接方式

1. 在 GitHub 创建一个 **Private** 仓库，并初始化 README，使选择的分支已经存在。
2. 建立 Fine-grained personal access token，只授权该仓库的 **Contents: read and write** 权限。
3. 打开 Lumen「设置 → 云同步」，选择「GitHub 私有仓库」，填写用户名或组织、仓库名、分支、仓库内目录及 Token。
4. 首次创建云空间时，同步密钥由 Lumen 本机生成并在连接后显示/复制。其他电脑填写同一仓库和分支，并粘贴这把 Lumen 密钥；Token 单独配置且只存于本机凭据库。

连接只接受私有仓库。仓库、Token 授权和 Lumen 加密密钥分别验证；换到另一同步空间时，必须提供正确的 Lumen 密钥才能读取已有内容。已有 WebDAV 配置缺少新字段时仍按 WebDAV 处理。

## 数据与冲突

业务文件在离开本机前由现有同步协议加密；GitHub 收到的是加密负载。同步版本、设备发现、上传队列和并发冲突沿用 Lumen 的版本链，冲突内容在应用内保留和处理。GitHub 提交历史额外记录文件更新，可在 GitHub 侧回溯，但不能代替 Lumen 的冲突选择。

GitHub Contents API 每次创建或更新文件都会产生仓库提交。单文件采用不超过 45 MiB 的数据块，先写入按 SHA-256 命名的内容块，再发布含整体大小、分块摘要与路径的清单；读取时逐块校验并校验整份内容。单个逻辑文件上限为 400 MiB，超过时拒绝该次上传且保留本机待同步数据。仓库历史会保留旧提交，GitHub 仓库大小会随长期修改增长。

GitHub Fine-grained token 权限由 GitHub 的 Contents API 文档定义；GitHub 普通仓库文件有 100 MiB 限制，因此 Lumen 将同步内容拆成小于此上限的文件。[Contents API](https://docs.github.com/en/rest/repos/contents) · [Fine-grained token 所需权限](https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens) · [GitHub 大文件限制](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github)

## 尚未验证

- 本地没有可用于连接的 GitHub 私有仓库 Token，因此尚未完成真实 API 授权、上传、下载和双电脑往返验证。界面不会代替用户创建仓库或申请 Token。
- 本轮可完成的本地前端检查已记录在 `docs/work-log.md`；Rust 下载 ONNX Runtime 构建依赖时本机曾长时间无进度，最终编译、单测与 Clippy 需以 GitHub CI 为准。
- 未实现 GitHub Enterprise、自托管 API 地址、网页 OAuth 或应用内创建仓库；Token 撤销后须在设置里重新连接。

