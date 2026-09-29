# 云同步官方接口调研

核实日期：2026-09-29。需求：多台电脑离线可编辑，联网后自动交换修改，冲突保留两边并提示。
用户没有既有网盘偏好，进一步明确第一优先是**备忘录和业务流程：家里编辑后及时上传，公司打开就能读**。
本轮交付调研、接口契约和协议验证模型，**未接通真实云账户，程序的云同步尚未实现**。

## 结论与推荐

建议首版先完成备忘录 / 顺序流程的即时同步，采用「本地 SQLite + Lumen 同步引擎 + 可替换的存储接口」，首个真实联调目标选
**坚果云 WebDAV**，随后验证 **OneDrive 应用文件夹**。这是工程建议，不是网盘速度或价格排名：
坚果云的官方流程只需账户和第三方应用密码；OneDrive 需要先由项目维护者注册 OAuth 应用，
之后终端用户只需浏览器登录授权。两种网盘都只负责存放同步数据，离线合并和冲突提示由 Lumen 做。

可借鉴 Joplin 已公开的离线优先、同步引擎与文件驱动分层，不复刻整个笔记软件。
Joplin 的同步设计还包含删除传播、同步状态与真实存储目标测试；仅连上网盘并不能自动获得这些能力。
[Joplin 官方同步设计](https://joplinapp.org/help/dev/spec/sync/)

| 路线 | 官方依据 | 对 Lumen 的判断 |
| --- | --- | --- |
| 坚果云 WebDAV | 应用密码、标准文件接口、官方列出 WebDAV 限额 | 建议首个联调目标；批量变更需要打包，不能每次轮询所有任务 |
| OneDrive / Microsoft Graph | OAuth、应用文件夹、分页、可恢复上传 | 建议第二个驱动；开发端应用注册比 WebDAV 多一步，授权范围更明确 |
| Nextcloud WebDAV | 官方文件接口与应用密码 | 若已有 NAS / Nextcloud 可复用 WebDAV；没有服务器的用户不应为此承担运维 |
| Dropbox API | OAuth PKCE、App Folder | 技术上可行，仍需应用注册；用户没有既有账号时不增加首版驱动数量 |
| 云客户端的本地交换目录 | Lumen 只读写协议文件，由云客户端传输 | 可以免去 Lumen 自行授权，但多一个客户端与运行依赖；仍然需要完整同步引擎 |
| Syncthing 交换目录 | 电脑间文件同步、冲突副本 | 可选无中心云路线；公司与家里电脑轮流开机时需额外常在线节点 |
| Supabase 等专用后端 | PostgreSQL 变更订阅及自行托管 | 适合将来多人协作；不是现有 SQLite 离线同步的现成替代品，首版不新增账号服务器 |

这里没有对国内网络可达性、速度、收费套餐或免费容量作未经实测的比较；最终要在用户实际网络
中做小文件联调。未要求用户现在注册、购买或提供密码。

## 一、OneDrive

### 授权与权限

- 使用用户委托授权，优先申请 `Files.ReadWrite.AppFolder` 与 `offline_access`。
- 应用文件夹入口：`GET https://graph.microsoft.com/v1.0/me/drive/special/approot`。
  `approot` 区分大小写；目录一般位于 `Apps/{注册的应用名称}`，首次访问可创建它。
- 官方应用文件夹说明覆盖个人与工作 / 学校 OneDrive。应用对文件夹具有权限边界，用户仍能
  自行改动或删除其中的文件，不能假定它永远存在。

[微软应用文件夹指南](https://learn.microsoft.com/en-us/graph/onedrive-sharepoint-appfolder)

维护者需注册公共客户端，选择所需账号类型、登记回调 URI，取得公开 `client_id`。
桌面端采用系统浏览器、Authorization Code + PKCE S256、一次性 `state` 与受限 loopback 回调；
不得把 `client_secret` 打进安装包。长期刷新令牌存系统凭据管理器，访问令牌仅在后端内存使用。
具体回调地址必须与注册匹配，不在所有网卡上监听授权码。
[应用注册](https://learn.microsoft.com/en-us/entra/identity-platform/howto-create-service-principal-portal)、
[授权码与 PKCE](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow)、
[回调 URI 限制](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url)

微软推荐受支持的认证库；当前官方认证库列表未列出 Rust MSAL。实现时需要选用有维护的 OAuth
库并测试桌面回调，或评估 MSAL 的额外运行时成本；不能宣称现有 Rust 代码已得到 MSAL 支持。
[官方认证库列表](https://learn.microsoft.com/en-us/entra/identity-platform/reference-v2-libraries)

### 文件接口与容易误判的地方

| 操作 | 官方接口 / 规则 | Lumen 接入约束 |
| --- | --- | --- |
| 发现目录内容 | `GET /me/drive/items/{folderId}/children`；默认一页 200 项，返回 `@odata.nextLink` | 逐页完成后再确认扫描结束；下一页 URL 视为不透明游标并校验来源 |
| 小文件上传 | `PUT /me/drive/items/{parentId}:/{filename}:/content`；单次上限 250 MB | 此 API 文档未说明 `If-Match`，不能把它当作已验证的比较并交换接口 |
| 可恢复上传 | `POST .../createUploadSession`，随后向 `uploadUrl` 顺序 PUT 字节范围 | 支持 `conflictBehavior=fail`；分片小于 60 MiB，通常按 320 KiB 对齐；最终片段边界单独处理 |
| 条件 / 冲突 | 创建上传会话文档列出 ETag 条件，失败返回 412；最终提交也可能因文件名冲突失败 | 会话创建时的条件不能直接证明所有提交场景都具有并发互斥，需要竞争测试 |
| 下载 | `GET /me/drive/items/{id}/content` 会返回 302 及短时预授权 URL；支持条件读取 | 不把 Bearer 转发到预授权下载地址，不记录完整 URL；限制响应大小与重定向 |
| 节流 | 429 按 `Retry-After` 等待；没有时再指数退避 | 失败保留待上传队列，不能持续立即重试 |

来源：[分页](https://learn.microsoft.com/en-us/graph/api/driveitem-list-children?view=graph-rest-1.0)、
[小文件上传](https://learn.microsoft.com/en-us/graph/api/driveitem-put-content?view=graph-rest-1.0)、
[上传会话](https://learn.microsoft.com/en-us/graph/api/driveitem-createuploadsession?view=graph-rest-1.0)、
[下载](https://learn.microsoft.com/en-us/graph/api/driveitem-get-content?view=graph-rest-1.0)、
[节流](https://learn.microsoft.com/en-us/graph/throttling)

`uploadUrl` 也是凭据：上传分片时不加最初 Graph 请求的 Authorization 头，过期后重建会话。
新协议文件采用不变名称与失败而非覆盖的创建语义；重复请求先检查已存在文件的摘要。

Graph delta 具有 `nextLink` / `deltaLink`、删除标记与令牌失效后的重扫行为。它返回的是
**文件变化**，并不是任务级合并。官方 delta 与部分文件 API 的权限表没有列出 AppFolder，
本轮未实测该最小范围对所有路径的覆盖；不因想用 delta 自动扩大到整盘权限。
首版应先验证应用文件夹内的已知路径读取、完整分页与创建，再将 delta 作为能力可选项。
[微软 delta 文档](https://learn.microsoft.com/en-us/graph/api/driveitem-delta?view=graph-rest-1.0)

## 二、坚果云 WebDAV

官方说明的接入流程是「账户信息 → 安全选项 → 第三方应用管理 → 新增应用 → 生成密码」。
使用账户邮箱与**第三方应用密码**，不是网页登录密码。服务地址为
`https://dav.jianguoyun.com/dav/`（HTTPS 443）。Lumen 仅操作自己的子目录。

官方帮助文章列出的限额：上传文件最大 500M；免费账户每 30 分钟 600 次请求，付费账户
1500 次；单次列目录最多 750 项（文件与文件夹合计），说明支持分多次获取，但没有公布
可直接照抄的分页参数。文章发表于 2021-01-05，不能把它当作本轮对真实账号限额的测量。

原文：[第三方应用授权文章](https://help.jianguoyun.com/?p=2064)。本轮直接访问该地址两次超时，
实际读取的是官方 [WebDAV 汇总页](https://help.jianguoyun.com/?tag=webdav) 上的完整同文，
以及该页第三方配置示例；没有用非官方教程填补这些事实。

### 需要真实账号验证的能力

WebDAV 标准包含 `PROPFIND`、`MKCOL`、`GET`、`PUT`、`MOVE`、`DELETE` 与条件请求。
标准强调强 ETag 的覆盖保护，但这不能证明某家服务实现的全部细节。
[RFC 4918](https://www.rfc-editor.org/rfc/rfc4918)

必须在 Lumen 专用测试目录验证：

1. 身份验证、目录读写、Unicode 名称与路径转义。
2. 207 Multi-Status 内各个响应 / 属性的状态，不能仅凭外层 207 判全部成功。
3. `If-None-Match: *` 是否拒绝覆盖；`If-Match` 错误 ETag 是否返回 412。
4. 强 ETag、条件读取和写后读的一致性；不依赖未经验证的 `LOCK` 或原子 `MOVE`。
5. 750 项之后如何完整发现数据。首版协议每个 journal 分桶最多 256 个文件并直接计算桶路径，
   避免依赖官方没有说明的分页扩展；遇到截断仍要报错而不是漏读。
6. 429 / 服务商限流响应、配额不足、上传超时后文件实际上已写入的情况。

Basic 认证只经 HTTPS 发送，默认不跟随跨主机重定向，不关闭证书校验。WebDAV XML 用真正的
命名空间解析器，不用正则；拒绝外部实体与目录越界的 `href`。应用密码只写系统凭据管理器，
不能保存在连接 URL、普通设置、JSON 备份、日志或前端 localStorage。

坚果云文档没有承诺像 OneDrive AppFolder 那样的凭据目录隔离。程序只访问专用目录，是
程序行为限制，不能描述为应用密码只对这个目录有效。

## 三、其它路线的官方依据

### Nextcloud

文件路径通常为 `/remote.php/dav/files/{user}/...`，官方支持 Basic 或有效会话，某些身份策略
需要应用密码。其 `oc:` / `nc:` 属性是扩展；通用驱动应只要求必要的 `DAV:` 属性。
不能把 Nextcloud 的分页、锁或属性能力套到坚果云。
[Nextcloud 官方 WebDAV 文件操作](https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/basic.html)

### Dropbox

官方提供 App Folder / Full Dropbox 权限选择，要求先注册应用；PKCE 可用于不能保守密钥的
客户端。可作为未来驱动，不为尚无 Dropbox 使用需求的首版增加授权实现。
[Dropbox 官方 OAuth 指南](https://developers.dropbox.com/oauth-guide)

### Syncthing 与本地交换目录

Syncthing 官方说明它在设备同时在线时交换数据，而不是替用户存到中心云；冲突文件也会继续
传播。因此可以运输 Lumen 的不可变同步包，不能直接代替任务冲突 UI。
[官方 FAQ](https://docs.syncthing.net/users/faq)、[冲突与传输机制](https://docs.syncthing.net/users/syncing)

OneDrive / 坚果云桌面客户端或 Syncthing 的文件同步目录**不应包含正在使用的 `lumen.db`、
WAL、SHM、凭据或设备身份文件**。SQLite 官方说明文件锁和事务依赖相关状态，外部复制或破坏
这些配套文件会损伤一致性。交换目录只能放 Lumen 已经封装并校验的协议文件。
[SQLite 官方损坏风险说明](https://sqlite.org/howtocorrupt.html)

### Supabase / 专用服务端

Supabase 的 Postgres Changes 是 PostgreSQL 变更订阅，需要相应表、订阅和访问控制；本机
SQLite 的离线写入、重放、删除与冲突仍需要另做。自行托管还包括服务器、升级、备份和监控。
所以当前个人多电脑目标无需先增加自建服务器。
[Postgres Changes](https://supabase.com/docs/guides/realtime/postgres-changes)、
[自行托管责任](https://supabase.com/docs/guides/self-hosting)

## 四、真实联调清单及当前边界

| 项目 | 本轮结果 / 进入实现阶段的条件 |
| --- | --- |
| 官方接入方式 | 已读取上述官方文档；权限、限额与未知项分别记录 |
| OneDrive 应用注册 | 未完成，仓库没有可用 `client_id`；需维护者实际注册公共客户端 |
| 坚果云账户 | 用户目前未选定 / 配置；未取得应用密码，未执行真实 WebDAV 请求 |
| 授权、刷新、撤销、限流、配额 | 未验证，需要真实账户与隔离测试目录 |
| 自动合并、冲突、删除协议 | 已制定方案，验证模型仅验证因果版本语义，不代表产品完成 |
| 跨电脑及加密互通 | 未验证，需要两个独立 profile 与实现后的真实服务驱动 |
| 数据上传 | 本轮没有读取凭据、上传用户任务或改动生产数据库 |

实施方案见 [云同步设计](design-cloud-sync.md)，请求 / 响应契约见
[云同步接口](cloud-sync-api.md)。官方文件操作提供运输能力；最终以双电脑离线竞争、崩溃恢复
和真实账户测试为发布标准，不能以接口文档存在或 HTTP 200 作为「同步完成」。
