# 云同步接口契约（设计稿）

日期：2026-09-29。**以下接口尚未在 Lumen 注册或实现，不能调用它们执行真实同步。**
这是待实现的请求、响应和错误约定；不新增 HTTP 服务器。UI 经既有 Tauri IPC 调用 Rust，
后端再调用网盘接口。协议与边界见 [设计](design-cloud-sync.md)，官方 URL 见
[调研](api-research-cloud-sync.md)。

首版范围为备忘录与顺序流程。状态需显示 scope=`memos`，即时上传由本地自动保存 / 手动保存
的成功事务触发；启动、打开备忘页面 / 记录与回焦触发拉取。所有命令均为下述设计约定，
本轮未实现这些触发。任务等其它范围仍需单独集成，不能在界面宣称全量同步。

## 一、公共类型

```typescript
type Provider = 'jianguoyun' | 'onedrive' | 'webdav';
type SyncPhase = 'disabled' | 'needsAuthentication' | 'idle' | 'offline'
  | 'reading' | 'uploading' | 'applying' | 'backoff' | 'paused' | 'error';

interface SyncStatus {
  scope: 'memos';
  provider: Provider | null;
  targetId: string | null;        // 本地连接身份，防止把状态误认成另一账户
  workspaceId: string | null;
  accountLabel: string | null;    // 本地界面可用，日志只显示脱敏值
  phase: SyncPhase;
  pendingBatches: number;
  pendingAttachments: number;
  unresolvedConflicts: number;    // 独立于 phase：有冲突也可继续同步其它实体
  operationId: string | null;
  lastSuccessfulCheckAt: string | null; // 完整成功检查，而非「启动请求」时间
  lastSuccessfulUploadAt: string | null; // 区分本端发出与已检查其它端
  retryAt: string | null;
  lastError: SyncError | null;
}

interface SyncError {
  code: 'AUTH_REQUIRED' | 'FORBIDDEN' | 'OFFLINE' | 'THROTTLED'
    | 'QUOTA_EXCEEDED' | 'CAPABILITY_UNVERIFIED' | 'REMOTE_RESET'
    | 'INVALID_REMOTE_DATA' | 'UNSUPPORTED_PROTOCOL' | 'MISSING_CAUSAL_HISTORY'
    | 'DEVICE_ID_COLLISION' | 'STALE_PREVIEW' | 'STALE_CONFLICT'
    | 'PENDING_CHANGES' | 'BUSY' | 'VALIDATION_ERROR' | 'LOCAL_IO_ERROR';
  message: string;                // 脱敏，不回传含凭据的 URL / 响应正文
  retryAt: string | null;
}
```

核心读写、校验、凭据存储失败返回错误；不返回默认空数据或伪成功。HTTP 状态与业务错误映射
需按 provider 实现：例如 404 可能是上传会话失效，也可能是整个同步空间消失，不能统一当空目录。

## 二、IPC 命令

字段使用 camelCase；写操作须校验 targetId，workspace 切换不得复用旧作业。
密码只有用户在设置中输入时短暂作为一次性请求值，不出现在任何返回对象、监听事件或调试日志。

| 命令（拟定名称） | 请求 | 返回 / 行为 |
| --- | --- | --- |
| `cloud_sync_get_status` | 无 | `SyncStatus`，不读取 / 返回凭据正文 |
| `cloud_sync_configure` | 提供者配置，见下方 | `{targetId, credentialsStored}`；只配置，不自动上传用户数据；持久化失败返回错误 |
| `cloud_sync_begin_onedrive_login` | `{targetId}` | `{authorizationOperationId}`；后端系统浏览器流程，成功 / 取消 / 失败由状态反馈 |
| `cloud_sync_test_connection` | `{targetId}` | `{operationId}`；在唯一专用探测目录验证必要能力，不覆盖业务文件；读取与有限写探测明确显示 |
| `cloud_sync_preview_join` | `{targetId, workspaceId}` 或明确 `createNew` | `{previewToken, localCounts, remoteCounts, estimatedConflicts, safetyBackupRequired, warnings}` |
| `cloud_sync_commit_join` | `{targetId, previewToken}` | `{operationId}`；重新核对本地 / 远端版本，先安全快照再持久接入，过期预览报错 |
| `cloud_sync_start` | `{targetId}` | `{operationId}`；仅表示作业已受理。成功合并与上传须由最终状态确认 |
| `cloud_sync_pause` | `{targetId}` | `SyncStatus`；停止调度，当前未提交事务回滚，已提交结果保留 |
| `cloud_sync_list_conflicts` | `{targetId, cursor?, limit}` | `{items, nextCursor}`；稳定全序，以冲突 ID 作最终排序条件 |
| `cloud_sync_get_conflict` | `{targetId, conflictId}` | 完整本机候选、共同基线（若有）与解决令牌，不上传到 AI |
| `cloud_sync_resolve_conflict` | 见下方 | `{resolvedConflictId, newRevisionId}`；业务写入与新 outbox 同事务，不表示云端已送达 |
| `cloud_sync_disconnect` | `{targetId}` | `{credentialsDeleted, localDataRetained: true}`；有待上传修改先返回 PENDING_CHANGES 及处理路径；不删除网盘数据 |

### 配置示例（非真实账户 / 非可执行调用）

```json
{
  "provider": "jianguoyun",
  "endpoint": "https://dav.jianguoyun.com/dav/",
  "username": "用户在设置中填写的邮箱",
  "applicationPassword": "用户在密码输入框填写，仅存系统凭据管理器"
}
```

坚果云固定 endpoint。通用 WebDAV 可输入 HTTPS endpoint；禁止 userinfo、片段、意外查询
参数与目录越界，默认不向跨 origin 重定向发送凭据。OneDrive 配置不含账户密码，维护者注册
的公开 clientId 与回调配置必须真实存在才可以开启登录，缺失时明确标「尚未配置 OneDrive」。

endpoint、账户标签和 workspace 选择为本机连接数据；不得通过全量 settings 备份意外传播。
自动同步开关在 commit_join 成功后才生效，不能凭 test_connection 的 HTTP 成功开启。

### 冲突解决

```typescript
interface ConflictDetail {
  conflictId: string;
  entityKey: string;
  entityType: string;
  resolutionToken: string; // 当前候选 ID 集合 + 本地业务版本的比较令牌
  variants: Array<{
    revisionId: string;
    deviceLabel: string;
    displayedAt: string;   // 不用于判定胜负
    operation: 'upsert' | 'delete';
    payload: unknown | null; // 按 entityType 的白名单验证
  }>;
  canKeepBoth: boolean;
}

type Resolution =
  | {choice: 'variant'; revisionId: string}
  | {choice: 'merged'; payload: unknown}
  | {choice: 'keepBoth'; revisionIds: string[]};

interface ResolveRequest {
  targetId: string;
  conflictId: string;
  resolutionToken: string;
  resolution: Resolution;
}
```

候选不局限两份：三台以上可能有更多候选。删除候选展示为删除意图，不当空标题显示。
选择期间新候选或用户本地编辑到达时返回 `STALE_CONFLICT`，保留草稿；选择形成观察全部
当前候选的新因果版本。`keepBoth` 只有明确支持独立复制的实体才允许，关联对象不可假复制。

## 三、后端存储驱动契约

下面是行为接口，尚无 trait / provider 实现。复用现有 reqwest，不让 React 直接请求云端。
支持 OneDrive 与 WebDAV 两条真实路线后再定 Rust 封装形式，不为单个驱动加入复杂插件系统。

| 方法 | 输入 / 输出 | 必须满足的条件 |
| --- | --- | --- |
| `probe` | 专用探测目录 → `Capabilities` | 明确区分官方已说明、运行时已测、未知；无凭据 / 权限时返回错误 |
| `listChildren` | 相对目录、opaque cursor → items / nextCursor / completeness | 限制在 workspace；完整分页 / 截断检测；单页不当作全量 |
| `read` | 相对路径、可选读取 ETag → bytes / etag 或 NotModified | 限流、大小上限、摘要校验；404 与整个目标不存在区分 |
| `createImmutable` | 唯一路径、冻结字节、期望摘要 → Created / AlreadyIdentical | 绝不替换不同字节；响应丢失后读回验证再确认；创建条件未知则禁止依赖它 |
| `writeDeviceHint` | 当前设备路径、版本提示 | 单设备写入；提示不是任务数据真相，失败不可把发送中的业务队列标已完成 |

首版不暴露任意远端永久删除方法。云端垃圾回收需有独立保留 / 退役 / 检查点协议后加入。
`Capabilities` 至少包含：条件创建是否验证、完整列目录能力、对象大小限制、条件读取能力、
可恢复上传、可选 delta、最近探测结果。不假装所有 WebDAV 提供者相同。

## 四、同步批次契约

以下为解密后的逻辑内容示例；真实云文件必须使用经审核的加密信封，不能直接上传此 JSON。

```json
{
  "protocolVersion": 1,
  "workspaceId": "示例空间 UUID",
  "deviceId": "示例公司电脑 UUID",
  "sequence": 7,
  "seen": {"示例公司电脑 UUID": 6, "示例家里电脑 UUID": 3},
  "batchId": "冻结后重试沿用的 UUID",
  "previousCipherDigest": "前一批次的密文摘要",
  "changes": [
    {"entityKey": "task:示例任务 UUID", "operation": "upsert", "payload": {"title": "示例标题"}},
    {"entityKey": "taskTag:示例任务 UUID:示例标签 UUID", "operation": "delete", "payload": null}
  ]
}
```

payload 的示例仅为便于阅读，真实完整实体字段以正式版本 schema 为准；未知版本、重复
entityKey、超限序号 / 字节数、因果缺口、非法关系、跨空间数据、摘要 / AEAD 不符均拒绝。
sequence 是当前设备连续批次序号，seen 的本设备值必须是 sequence-1。一次批次覆盖一次
完整业务事务，业务实体的 revision 来源是 `{deviceId, sequence}`。事件 / payload 校验不能
只做 TypeScript 类型断言，须在 Rust 的不可信输入边界真实执行。

## 五、状态事件与完成语义

拟定事件 `cloud-sync-status-changed` 只携带 `{targetId, operationId, status}`；前端复用现有
异步监听清理工具，卸载页面后不保留旧注册。重连 / 打开设置先重新查询 status，不能依赖
可能丢失的事件。网络作业只有一个，重复 start 返回 BUSY 或现有 operationId，不启动第二个。

「本机本次已同步」表示本地队列已持久确认，且本次完整读取的远端批次已事务处理。不能表示
离线设备没有未上传内容；显示最后成功检查时间。有冲突时明确提示候选未解决，不以任务行
当前显示值掩盖它。网络断开仍允许本地操作，队列失败只影响同步状态。

## 六、接口验收边界

本轮没有注册任何上述 Tauri 命令、存储驱动或后台任务。协议验证模型只验证因果版本；
真实登录、Windows 凭据存储、HTTP / XML、SQLite 事务、加密、附件、重复系列与 UI 尚未实现。
这些验收必须使用实现后的生产代码和隔离数据，不能返回固定示例对象来充当接口实现。
