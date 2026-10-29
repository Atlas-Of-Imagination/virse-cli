# Virse CLI

Rust 命令行客户端，面向交互使用、脚本和 Agent。支持 API Token 认证、资源与权限管理、持久导入批次、画布节点/组合/连线与讨论、异步生成、智能工具创作与运行、ZIP 交付和断点下载。

## 架构

```text
virse-cli（bin: virse）
    参数、环境变量、Profile、凭据文件、恢复记录、输出
                     ↓
virse-api（异步 lib）
    类型化请求与任务、Bearer 认证、HTTP、WebSocket、错误
                     ↓
Virse Backend
```

| 职责 | `virse-api` | `virse-cli` |
| --- | --- | --- |
| 配置 | 显式接收 Backend 地址与 Token | 解析环境变量与 Profile |
| API | 请求、响应、权限错误、资产流式下载 | 命令和参数转换 |
| 生成 | 提交、查询、观察、取消 | 同 key 重试、等待超时、Ctrl+C、恢复记录 |
| 持久化 | 不读环境、不读写用户文件 | 凭据、默认组织/画布、提交身份 |
| 输出 | 返回数据、错误、任务快照流 | stdout 结果、stderr 进度/错误、退出码 |

仓库根是 `virse-cli` package，`virse-api/` 是可独立依赖的 SDK。仅这两个 crate，不依赖前端代理、MCP 或浏览器登录。

## 安装与配置

```bash
cargo install --path . --locked --jobs 2
virse --help
```

Backend 默认 **`https://api.virse.ai`**，**只能用环境变量覆盖**：

```bash
export VIRSE_BACKEND_URL=https://dev4.virse.ai
```

不支持 `--backend`、`--backend-url` 或 Profile 中的 Backend 地址。变量未设置才使用 production；空值、非法 URL、含账号/密码、查询参数或路径的地址会报错。HTTP 和 WebSocket 使用相同 origin，自动将 HTTPS 转换为 WSS。

| 环境变量 | 用途 |
| --- | --- |
| `VIRSE_BACKEND_URL` | Backend origin；未设置时使用 production |
| `VIRSE_API_TOKEN` | 当前进程 API Token，优先于保存的凭据 |
| `VIRSE_CONFIG_DIR` | 覆盖本地状态目录，便于 CI 和隔离测试 |

未指定状态目录时使用系统用户配置目录，例如 Linux 的 `$XDG_CONFIG_HOME/virse` 或 `~/.config/virse`。`--profile <name>` 选择命名 Profile，默认为 `default`；凭据、组织和画布默认值均按 **Profile + Backend** 隔离。Backend 身份用于隔离记录，不是可保存的地址配置。

```bash
virse auth login                    # 隐藏输入 virse_sk_… Token
# 自动化使用 VIRSE_API_TOKEN，或通过 stdin 导入：
secret-manager-command | virse auth login --token-stdin
virse auth status
virse config set org <org-uuid>
virse config set canvas <canvas-uuid>
virse config get
```

登录通过 `/auth/me` 验证成功后保存 Token。Unix 状态目录权限为 `0700`、文件为 `0600`，使用原子替换和写锁。凭据保存在本地文件，尚未接入系统钥匙串。非交互登录必须使用 `--token-stdin`，不提供明文 Token 参数。

`auth logout` 仅删除当前 Backend/Profile 的本地凭据，不撤销服务器 Token；如果仍设置 `VIRSE_API_TOKEN`，它仍然生效。

## 空间、模型与余额

```bash
virse space list --json
virse config set org <org-uuid>
virse model list --mode image --mode video --json
virse model list --favorites --include-disabled --json
virse model get <model-key> --json
virse model favorite <model-key>
virse model unfavorite <model-key>
virse balance --json
```

空间列表返回服务端提供的个人/团队类型和 `my_role`，不自动改变默认组织。模型查询要求明确的组织（`--org` 或 Profile 默认值），默认读取图片、文本、视频和 3D 四类；保留可用性原因、参数限制、价格与收藏信息。`get` 返回同一 model key 的所有模式条目。模型目录价格不替代一次具体任务的有效报价；余额/预算中的未知或不限额值保持原样，不按 0 推断。

收藏更新仅发送一次；结果不确定时退出 7，先重读目录确认。余额查询不购买、不调整预算。

## 创建画布与读取内容

```bash
virse canvas create '新画布' --parent '/项目' --json
virse canvas contents '/项目/新画布' --kind document --document-type pdf --limit 50 --offset 0
```

创建返回 `canvas_space_id` 与目录资源 `dentry.id`；需要默认画布时显式执行 `config set canvas`。创建接口没有幂等合同，响应不确定时先列目标目录，不自动重发。`contents` 的目标是**画布资源路径或资源 ID**，不是画布 Space ID；返回媒体/文档/审美的 Content 投影，与节点布局分开。支持名称搜索、类型、文档子类、排序及 offset 分页，`has_more` 时继续读取；不是快照分页。

## 像文件系统一样管理资源

资源以当前组织为根目录，文件夹可以嵌套；画布、图片、视频、文档等作为资源项显示。路径区分大小写，含空格时按 Shell 规则加引号。每条命令从组织根目录开始，不改变本机工作目录，不维护隐式远端 `cd` 状态。

```bash
virse config set org <org-uuid>
virse resource ls /
virse resource ls '/素材' --kind image
virse resource tree / --depth 3
virse resource tree '/素材' --ids
virse resource search '产品' '/素材' --kind image --json
virse resource get '/素材/产品图.jpg' --json
virse resource download '/素材/产品图.jpg' --output product.jpg
```

`tree` 默认展示名称和层级；`--ids` 同时显示 ID，`--json` 返回嵌套节点。默认最多显示 3 层、读取 2000 项，可通过 `--depth` 和 `--max-items` 调整。达到边界会明确标记未完全展开，不把部分结果当成整棵树。分页返回中父目录未包含的资源单列展示，不猜测其位置。

`ls` 默认列直接子项，一页 100 项；`--limit` 上限 200，使用返回的 `next_cursor` 配合 `--cursor` 续页。`--recursive` 列后代，`search` 默认递归搜索。支持 `--view all|mine|team-shared|shared|external` 和重复 `--kind` 过滤；共享入口可从列表取得 ID，再直接浏览对应文件夹。续页必须保持同一目录和筛选条件，过期游标需重新查询。

### 路径与 ID

所有资源操作均可通过 `/文件夹/名称` 或 `@<resource-uuid>` 指定资源，也兼容裸 UUID。根目录写作 `/`；相对路径、`.`、`..`、通配符与快捷方式目录穿透暂不支持。

- 后端允许同目录重名。路径解析会读取完整目录的所有页；存在多个同名项时返回候选，不挑选第一个，也不只凭文件类型猜测。
- 出现歧义、名称包含 `/`，或不可通过可见祖先访问时，使用列表或详情中的 `@UUID`。
- 只有文件夹能作为目标目录。画布在资源树中是一个文件项，画布元素操作继续使用 `canvas` / `generation` 命令。
- 资源下载会自动解析文件和访问上下文，不需要指定画布 ID 或资产 ID。

### 筛选、排序与预览

```bash
virse resource ls '/素材' --sort modified-at --order desc --created-by-me --access edit --access manage
virse resource preview '/素材/a.png' --output preview.jpg
virse asset preview <asset-uuid> --canvas <canvas-uuid> --output preview.jpg
```

`--sort name|created-at|modified-at|size|custom`、`--order asc|desc`、`--created-by-me` 和可重复的 `--access view|use|edit|manage` 同样适用于 `search`。当前只支持“本人创建”筛选，不提供任意创建者 UUID 参数；续页保留相同条件。

预览默认 `--representation preview`，取真实图片缩略图、视频/模型封面或文档展示内容；需要视频/模型交互展示字节时可显式选择 `content`。不存在的封面直接报错，不回退到原件；保留版本参数、同源认证和原子文件写入规则。输出文件扩展名不会改变格式。

### 创建、改名、移动、复制

```bash
virse resource mkdir '/素材'
virse resource mkdir '/素材/产品图'
virse resource mkdir '新文件夹' --parent @<folder-uuid>
virse resource rename '/素材/产品图' '产品摄影'
virse resource move '/素材/产品摄影' '/归档'
virse resource copy '/归档/产品摄影' '/素材'
virse resource move '/素材/a.png' '/素材/b.png' '/归档'
virse resource copy @<id-1> @<id-2> '/素材'
```

`mkdir` 一次创建一个文件夹，父目录必须存在；同名项已存在时拒绝创建。`rename` 的第二个参数是新名称，`move` / `copy` 的最后一个参数是已存在的目标文件夹或 `/`，前面的一个或多个参数是来源，不把它解释成新文件名，不隐式覆盖或合并资源。

移动可能改变哪些人能访问资源。后端要求确认时，CLI 返回变化详情并停止；核对后可显式使用 `--confirm-access-change` 重发。确认后的操作使用新幂等 key，不自动接受权限变化。

### 回收站与恢复

```bash
virse resource delete '/素材/旧图.jpg'
virse resource delete @<resource-uuid-1> @<resource-uuid-2>
virse resource trash --json
virse resource trash --scope '/共享文件夹' --json
virse resource restore @<resource-uuid>
```

`delete`（别名 `rm`）将资源移入回收站；文件夹连同内容处理，与 Web 行为一致。`restore` 使用回收站返回的 ID，恢复位置和访问权限以服务端结果为准。永久删除与分批清空见 [资源工作流](docs/CLI-WORKFLOWS.md)，必须明确确认并检查逐项结果。回收站列表沿用 Web 的可见范围，不能用它证明所有历史删除记录均已列出。

### 失败与重试

改名、移动、复制、删除和恢复均使用后端统一幂等接口，支持 `--idempotency-key`。CLI 在发送前冻结账号、组织、已解析的资源与目标目录；自动重试以及同 key 手动恢复使用这一份请求。因此即使原路径已经改名或删除，同一条命令重放也不会重新选中其他同名资源。

```bash
virse resource rename '/素材/a.jpg' 'b.jpg' --idempotency-key rename-001 --json
# 响应丢失时，重复原命令和 key；不要将源路径改成新名称。
virse resource rename '/素材/a.jpg' 'b.jpg' --idempotency-key rename-001 --json
```

恢复记录保存在当前配置目录，按 Backend、账号、组织和 key 隔离，含资源 ID、操作参数与指纹，不含 Token。不要在请求结果未知时删除记录。换设备后如没有原记录，应使用原资源 ID 确认状态，不能猜测旧路径对应的资源。

批量结果逐项报告；任一项失败时返回退出码 `6`，不重复执行已成功项。确定失败项需要重新执行时，只提交这些项并使用新 key。网络错误或响应损坏后的最多三次自动重试始终使用原请求；结果仍未知时返回退出码 `7` 和 key。

文件夹创建目前没有后端幂等保障，因此不自动重试。响应丢失时先列父目录确认，CLI 会明确报告“创建结果待确认”。

### 上传与替换内容

统一上传/替换需要后端部署资产内容 API（后端 PR #612）并完成对应迁移。旧环境不自动改走旧上传入口。

```bash
virse resource upload ./photo.png '/素材' --conflict rename --json
virse resource upload ./photo.png @<folder-uuid> --idempotency-key upload-001
virse resource get '/素材/photo.png' --json
# expected-revision 取自上一步 asset.content.revision，不是画布或模型编辑版本。
virse resource replace '/素材/photo.png' ./edited.png --expected-revision 3 --json
```

单次上传一个本地普通文件，目标是已有目录或 `/`。支持图片、视频、DOCX、PDF、GLB，单文件最多 **100 MiB**，实际格式由服务器检查；不靠改扩展名转换格式。上传完成才返回成功，普通文件上传不创建生成任务，也不自动放置画布元素。可以再用既有 `canvas add --resource` 放置。

`--conflict rename|skip|error` 默认为 `rename`。`skip` 的 `skipped=true` 只说明同名跳过，不能说明服务器字节与本地一致。命令返回服务器的资产、资源项和内容版本，不建立本地文档同步绑定。

替换保留资产 ID，要求相同资产类型，Word/PDF 不能互换。`--expected-revision` 必须显式提供；当前版本不一致直接报冲突，不自动更新版本重试。GLB 若需要清除既有编辑状态，必须同时传 `--reset-model-state --expected-state-revision <读取到的模型编辑版本>`；此命令不代替 3D assembly 保存。

上传与替换在提交前保存原始字节、文件名、账号、组织、目标 ID、请求参数和幂等 key。状态按 Backend/账号/组织/key 隔离，私有文件保存在配置目录，进度写 stderr，结果写 stdout。响应丢失时重复原命令与同一个 key；普通重试检查本地内容未变，且不会重新解析已经移动或改名的资源路径。

```bash
# 本地原件已移动或删除时，明确使用已保存的字节恢复。
virse resource upload ./photo.png '/素材' --idempotency-key upload-001 --resume
```

`--resume` 要求既有恢复记录和原 key，使用冻结的请求，不读取当前本地文件。已完成操作也向后端确认；如果文件后来又被修改，保留 `content_changed` / `revision_conflict` 及完成版本详情，不返回过期的缓存成功。结果未知时退出码为 `7`，不要换 key 盲目新建或删除可能已经提交的资源。恢复记录包含文件副本，可能占用与上传文件相近的本机空间。

下载保持后端 URL 中的版本参数，检查详情与下载描述的内容版本；服务端提供大小或 SHA-256 时，校验通过才发布目标文件。未知校验和保持未知，不编造值。内容冲突原样报告；已有目标文件不会被覆盖。

### 实时观察资源变化

```bash
virse resource watch --json
virse resource watch --cursor '<上次处理完的 next_cursor>' --json
virse resource watch --timeout 60 --json
```

观察当前组织，JSON 模式逐行输出事件页，每页带 `type=page`、`items`、`next_cursor`、`has_more` 和 `bootstrap`；处理完整页后才保存其游标。重连提示为 `type=notice`，不作为新的事件检查点。无 cursor 时先输出当前基线，只观察此后变化，不自动输出一份初始资源树。CLI 不缓存资源，也不隐式保存观察游标。

需要初始快照的消费者先读取 `resource ls --json` 的全部分页，再把同一快照返回的 `sync_cursor` 交给 `watch`，不能在全量结束后另取最新水位。`next_cursor` 是目录分页游标，`sync_cursor` 才是变更游标；独立执行的多次 `tree` 查询不构成一个全局快照。

WebSocket 只唤醒 HTTP 按游标补齐；连接就绪、收到通知和重连时补齐到 `has_more=false`，没有周期 HTTP 轮询。API Token 仅放握手 Header。网络恢复次数有限，失败保留最后输出的 cursor 供再次续接。`--timeout` 到期正常退出，Ctrl+C 退出 `130`。

`scope_invalidated` 表示应丢弃该组织的消费者缓存并重新读取；CLI 不推测隐藏对象。`cursor_expired` / `sync_reset_required` 要求调用者重新基线和全量读取，权限撤销则停止观察。后端未开启 changes 时明确返回 `changes_not_enabled`，不做轮询降级。

SDK 同时提供 `resolve_asset_context`、`asset_detail`、`assets`、`upload_asset`、`replace_asset_content`、`presentation_info_in`、`changes` 和 `watch_changes`。上传接收显式字节与参数，SDK 不读取文件、不存凭据或游标、不自动重试写入。展示描述仍用安全下载传输消费，保留 representation 与版本参数；展示读取不会提升原件下载权限。

## 资源权限与分享

权限和分享同样使用资源路径或 `@UUID`，`--org` 是当前操作身份所在的工作空间。

```bash
virse resource permissions get '/素材' --json

# 先查看影响，再明确执行；--dry-run 不修改权限。
virse resource permissions set '/素材' --member <user-uuid> --role use --dry-run --json
virse resource permissions set '/素材' --member <user-uuid> --role use --json
virse resource permissions set '/素材' --member '<user-uuid>@<org-uuid>' --role edit
virse resource permissions set '/素材' --group <group-uuid> --role view
virse resource permissions set '/素材' --email collaborator@example.com --role view
virse resource permissions set '/素材' --org-members --role view
virse resource permissions set '/素材' --link --role none

# rule UUID 来自 permissions get 返回的 rules[].rule_id。
virse resource permissions remove '/素材' <rule-uuid> --dry-run --json
virse resource permissions remove '/素材' <rule-uuid> --json
virse resource permissions clear '/素材' --dry-run --json
virse resource permissions clear '/素材' --json
```

| 角色参数 | 含义 | 可用于 |
| --- | --- | --- |
| `none` | 无本层授权 | 全体成员、链接 |
| `view` | 可查看 | 所有授权对象 |
| `use` | 可使用 | 所有授权对象 |
| `edit` | 可编辑 | 所有授权对象 |
| `manage` | 可管理 | 成员、邮箱 |

一次 `set` 选择一个对象：`--member`、`--group`、`--email`、`--org-members` 或 `--link`。裸成员 UUID 使用当前 `--org` 作为该成员的工作空间身份；跨工作空间身份用 `UUID@ORG_UUID` 明确指定。`--org-members` 始终作用于资源所属组织的全体成员，不是把当前会话组织添加到资源上；个人空间没有该受众，后端会明确拒绝。

`get` 返回完整规则、继承来源、我的有效权限，以及 `can_manage`、`max_role`、`removable` 等服务端能力。只能查看者仍能读取名单。CLI 不改写权限求解逻辑，不允许修改继承/创建者行，也不通过降低本层权限伪装成已经关闭继承访问。

`set` / `remove` 在写入前读取当前名单并调用服务端影响预览；预览失败时停止写入。加 `--dry-run` 只返回拟变更和影响，正常命令显式执行一次变更。预览和写入不是原子事务，以服务端返回的新状态为准。`remove` 成功后重新读取名单；如果写入成功但刷新失败，仍报告 `applied=true`，并附带 `refresh_error`，不会误报成写入失败。

`clear` 只清除当前资源本层规则并将本层链接权限设为零，不删除资源、不作废地址，也不清除继承规则。其 `--dry-run` 列出当前设置，不编造合并后的受影响人数。执行成功后返回当前完整名单，可能仍然存在有效访问权。

分享写接口尚无请求级幂等回执，本轮不提供 `--idempotency-key` 或自动重试；断线、超时或损坏响应导致结果未知时退出码为 `7`，先重读名单再决定下一步。

### 分享链接与认领

```bash
# 不猜测前端域名：默认保留后端返回的相对地址。
virse resource share link '/素材' --json
# 明确前端 origin 时输出完整浏览器链接。
virse resource share link '/素材' --web-origin https://www.virse.ai

virse resource share resolve 'https://www.virse.ai/s/<token>' --json
virse resource share resolve 'https://www.virse.ai/canvas/<canvas-uuid>?share=<token>' --json
virse resource share claim 'https://www.virse.ai/s/<token>' --org <receiving-org-uuid> --json
virse resource share claim --resource <resource-uuid> --org <receiving-org-uuid> --json
```

获取链接不会赋予访问权。后端可能在读取设置时生成一个权限为零的地址；实际访问仍需要登录并通过权限校验。CLI 保留后端实际返回的地址形式，支持普通资源 `/s/<token>` 和画布 `/canvas/<uuid>?share=<token>`，不将画布地址重新拼成其他形式。可用 `--web-origin` 显式指定 Web 域名；它不改变 Backend 配置。

解析支持原始 token、相对地址和完整 HTTP(S) URL，始终向已配置的 Backend 发请求，不访问粘贴 URL 的主机。画布地址中的画布 ID 与分享 token 一起传给后端校验。返回 `claimable=true` 只表示存在可认领邀请，不自动接受；必须显式运行 `claim`，邮箱是否匹配、所选工作空间是否可用均由后端判断。

邮箱授权创建待接受记录，不代表对方已经获得权限；本轮 CLI 不发送邮件。`claim` 成功返回服务端定位与权限，可再次 `resolve` 刷新。用户组的创建/成员管理、组织级分享总闸管理不在本轮范围内。

## 从画布到生成资产

### 1. 读取画布和引用

```bash
virse canvas list --org <org-uuid> --limit 50 --json
virse canvas list --org <org-uuid> --cursor '<next_cursor>' --json
virse canvas get <canvas-uuid> --json
virse generation operations --canvas <canvas-uuid> --json
```

`canvas list` 使用带组织权限校验的 `/api/v1/resources/query`，只查询 `canvas`，支持游标分页。返回条目的 `id` 是资源目录 ID，**`target_id` 才是后续使用的画布 ID**。组织 ID 来自现有工作空间；首版不提供组织发现命令。

`canvas get` 返回项目和元素，使用元素 `id` 作为引用。操作 Schema 描述业务输入；模型 key 需使用目标环境模型目录中的实际值，CLI 不硬编码默认模型。

### 2. 一次请求放置元素并生成

```bash
virse generation image \
  --canvas <canvas-uuid> \
  --model <model-key> \
  --prompt '保留 $1 的主体，改为白色背景' \
  --reference <source-element-uuid> \
  --x 100 --y 200 --width 512 --height 512 \
  --wait --json
```

这是一个 `POST /v2/canvas/projects/{id}/generations` 请求，包含 `operation/input/outputs` 和 `Idempotency-Key`。默认通过 `outputs[].create_element` 指定占位；后端在同一事务中创建占位、任务、输出绑定和必要的额度预占，再异步生成并绑定结果。CLI 不提前单独创建元素，也不自行补绑结果。

- 默认新元素位置 `(0,0)`，显示尺寸 `1024×1024`，可以覆盖；显示尺寸不是模型输出分辨率。
- 重复 `--reference` 按输入顺序映射 `$1`、`$2`，传递 `canvas_element` 来源，不把引用转换为 URL。Shell 中含 `$1` 的提示词请使用单引号。
- 对已有元素生成使用 `--target-element <uuid>`，与位置和尺寸参数互斥。
- 生图快捷命令生成单张图片；默认立即返回已接受的任务快照，只有 `--wait` 才等待完成。

### 视频和 3D 生成

视频和 3D 与图片共用引用、布局、幂等提交、等待和恢复机制：

```bash
virse generation video --canvas <canvas-uuid> \
  --model wan-3.0 --prompt '围绕 $1 中的主体缓慢运镜' \
  --reference <image-element-uuid> \
  --parameters '{"duration":2,"resolution":"480p","aspect_ratio":"16:9","generate_audio":false}' \
  --wait --json

virse generation 3d --canvas <canvas-uuid> \
  --model tripo-h3.1 --prompt '一把方形座面的四脚木凳' \
  --parameters '{"texture":false,"geometry_quality":"standard","face_limit":3000}' \
  --wait --json

# 图片参考生成 3D 时可以省略提示词；图片顺序依次绑定 $1、$2……
virse generation 3d --canvas <canvas-uuid> \
  --model tripo-h3.1 --reference <image-element-uuid> --wait
```

三个快捷命令都只创建一个输出，可用 `--target-element` 指定已有的同类型元素。图片默认显示尺寸 `1024×1024`，视频 `512×288`，3D `512×512`；均支持 `--x/--y/--width/--height`。位置默认 `(0,0)`。显示尺寸与生成模型输出尺寸独立。

图片、视频和 3D 均支持 `--parameters '<JSON对象>'` 或 `--parameters-file params.json`，两者互斥。参数直接进入后端的 `input.parameters`；时长、分辨率、音频、几何质量、贴图等字段由所选模型校验，CLI 不硬编码供应商参数默认值。参数文件最大 1 MiB。请按目标环境实际模型目录选 key 和参数；示例不保证每个环境都启用了这些模型。

`--reference` 始终指画布元素 ID，按输入顺序映射引用键；视频首尾帧或多图 3D 的角色遵循后端所选模型规则。直接引用 3D 元素来生图/生视频所需的截图渲染，以及四视图生成/确认不属于本轮快捷命令。更复杂的引用和操作仍使用通用 JSON 提交。

### 将已有资产放入画布

```bash
virse canvas add --canvas <canvas-uuid> --org <org-uuid> \
  --resource '/素材/产品视频.mp4' --json

virse canvas add --canvas <canvas-uuid> --org <org-uuid> \
  --resource @<resource-uuid> --x 1200 --y 100 --name '参考模型'

virse canvas add --canvas <canvas-uuid> --org <org-uuid> \
  --asset <asset-uuid> --width 800 --height 450 --json
```

支持已有的图片、视频、GLB、Word 和 PDF，自动识别元素类型；Word/PDF 都使用文档元素。`--resource` 复用资源绝对路径与 ID 解析，`--asset` 直接指定资产 ID，二者必须且只能选一个。不接受文件夹、普通文本、审美或未就绪的资产。

这是一条创建画布元素的 HTTP 请求，不调用模型生成，也不上传文件。未指定布局时，由后端按类型决定尺寸并放在已有内容右侧；指定尺寸时宽高需一起传入。资源入口默认采用资源名称，`--name` 可覆盖；直接资产入口未指定名称时使用后端默认命名。

`canvas add` 支持 `--idempotency-key`。CLI 在发送前保存已解析的资产身份和创建参数；请求结果未知时，用原命令和同一 key 恢复，原资源改名也不会重新解析到别的资产。结果返回 `element` 及创建回执；后端可能为画布绑定独立资产，调用方应使用返回的 `element.content_ref`。

已有资产放置需要后端启用 `CANVAS_HTTP_COMMANDS_ENABLED`，生成需要 `CANVAS_GENERATION_TASKS_ENABLED`；未启用时明确返回错误，不回退旧接口创建第二份节点。Word/PDF 暂无模型生成操作，本轮只支持放置已有文件。

### 3. 通用提交

```bash
virse generation submit --canvas <canvas-uuid> --file request.json --json
cat request.json | virse generation submit --canvas <canvas-uuid> --file - --wait
```

`request.json` 示例（替换实际 model 和元素 ID）：

```json
{
  "operation": "image_generate",
  "input": {
    "model": "<model-key>",
    "user_query": "保留 $1 的主体，修改背景",
    "image_count": 1,
    "references": {
      "$1": {
        "source": {
          "kind": "canvas_element",
          "element_id": "00000000-0000-0000-0000-000000000001"
        }
      }
    }
  },
  "outputs": [{
    "output_index": 0,
    "create_element": {
      "position": { "x": 100, "y": 200 },
      "size": { "width": 512, "height": 512 }
    }
  }]
}
```

可以通过同一入口调用后端已支持的多图、文字及其他操作，参数以 `generation operations` 返回的 Schema 为准。图片、视频和 3D 的基础生成已有快捷命令；没有新增 3D 截图渲染能力。现有 API 仍要求明确输出绑定，通用提交不是独立于画布的资产生成接口。

### 4. 观察和恢复

```bash
virse generation get <task-uuid> --canvas <canvas-uuid> --json
virse generation watch <task-uuid> --canvas <canvas-uuid> --timeout 900 --json
virse generation cancel <task-uuid> --canvas <canvas-uuid>
```

观察通过现有 Canvas WebSocket 的任务订阅进行，接收当前快照与后续更新，按 revision 去重。初次观察及断线恢复时有限查询任务，重连后重新订阅，不周期轮询。

- 等待默认 600 秒，可用 `--timeout` 修改。超时或 Ctrl+C 仅停止本地等待，不取消服务端生成。
- `needs_attention` 结束本次等待并提示待核对，不伪装成失败，不自动重试供应商。
- 取消按后端真实能力执行；运行中任务不支持取消时保留服务器错误。
- 提交会在 stderr 输出幂等 key。首次请求加最多三次自动重试始终使用相同 key 和请求体。
- 本地记录仅保存 Backend、账号、画布、key、请求摘要与受理后的 task ID，不保存提示词或 Token 副本。恢复记录不自动重放。
- 请求受理结果不确定时，使用**同一请求**和 `--idempotency-key <原key>` 再次提交确认；不要换新 key。相同 key 搭配不同请求会被拒绝。新的显式提交默认产生新 key。

### 5. 获取资产

```bash
virse asset get <asset-uuid> --org <org-uuid> --canvas <canvas-uuid> --json
virse asset download <asset-uuid> --org <org-uuid> --canvas <canvas-uuid> --output result.png
```

资产 ID 来自任务的 `outputs[].asset_id`。文本等操作以任务 `result` 和画布内容为准，不能假设每个输出都有可下载资产。

下载先读取受权限保护的下载信息，再流式写入临时文件，成功后发布目标文件；已有文件不覆盖，跨源下载/跳转不携带 Backend Token。下载失败不会重新执行生成。

下载保留服务端原始字节，指定文件扩展名不会转换图片格式。

## 本地创作和运行智能工具

工具使用现有技能包格式：`SKILL.md` 定义执行行为，可选 `interface.json` 定义输入表单与输出，`meta.json` 保存作者元数据，参考资料和脚本随包交付。CLI 不在本机执行包内脚本。新增的普通作者包发布能力对应工作区 Issue #116，须先部署配套后端迁移与接口；旧环境仍可使用原有的工具查询和 V2 运行能力。

```bash
virse tool init ./my-tool --name 'My tool'
virse tool validate ./my-tool
virse tool publish ./my-tool --org <org-uuid>
virse tool publish ./my-tool --org <org-uuid> --resume
virse tool list --view created --org <org-uuid> --json
virse tool get <tool-id> --org <org-uuid> --json
virse tool versions <tool-id> --org <org-uuid> --json
virse tool pull <tool-id> --output ./downloaded-tool --org <org-uuid>
virse tool pull <tool-id> --output ./historical-tool --version 1.0.0 --org <org-uuid>
```

`init` 和 `validate` 是离线命令，不需要 Token 或 Backend 配置；后端仍会进行权威内容校验。首次发布默认个人范围，`--scope team` 显式选择团队；更新不显式传 scope 时保留已有范围，显式传入则与发布事务一起调整。仅改变范围不会增加正式版本。更新必须基于本地绑定的远端版本，不能自动覆盖 Web 或另一 CLI 的更新。存在活跃 Creator 编辑时，需要先处理原编辑会话。

发布沿用原有上传、草稿与正式发布体系，不需要 Creator Agent 再制作一遍本地完整包。正式版本首次为 `1.0.0`，后续由后端递增 patch。历史源码拉取与当前远端发布基线分别保存，历史内容重新发布产生新正式版本，不倒退或改写历史。

本地包限制 ZIP 不超过 20 MiB、展开不超过 100 MiB、最多 2048 项；拒绝路径穿越、别名/重复条目、符号链接、硬链接与特殊文件。根目录 `meta.json` 用于发布元数据，不进入执行包；嵌套同名文件仍作为附带资料保留。Git、凭据、依赖和构建缓存不进入包，校验输出会列出排除项。

当前版本化包更新支持名称、简介、详情、封面及包内执行说明/输入定义。其他声明字段若与已有发布元数据不同，会明确拒绝，不静默丢弃。

拉取仅允许作者读取自己的源码，默认拒绝覆盖已有目录。项目绑定和发布恢复记录按 Backend、账号、组织隔离，不进入发布包。网络中断后保留本次请求快照与阶段信息，用 `--resume` 继续原发布；不要删除恢复记录后反复创建新工具。

明确拒绝（例如参数错误或个人空间不允许团队范围）后，可以修正输入再发起发布；已经成功创建的草稿身份会保留，不重复创建工具。结果未知仍须先用 `--resume` 重放原快照。遇到过期基线时不会自动重基；若已有上传修订，可显式放弃后再拉取到新目录、合并本地改动：

```bash
virse tool draft abandon <tool-id> <revision-id> --org <org-uuid>
virse tool pull <tool-id> --output ./fresh-tool --org <org-uuid>
```

放弃草稿不改变当前正式版本。拉取安装中断时不会把半成品目录当作已绑定项目；保留现场并换一个空目标目录重试。

### 工具管理

```bash
virse tool list --view mine --org <org-uuid>
virse tool list --view store --query '白底图' --org <org-uuid>
virse tool scope <tool-id> personal --org <org-uuid>
virse tool scope <tool-id> team --org <org-uuid>
virse tool add <tool-id> --org <org-uuid>
virse tool disable <tool-id> --org <org-uuid>
virse tool enable <tool-id> --org <org-uuid>
virse tool star <tool-id> --org <org-uuid>
virse tool unstar <tool-id> --org <org-uuid>
virse tool remove <tool-id> --org <org-uuid>
virse tool delete <tool-id> --org <org-uuid>
```

`remove` 仅移出个人添加列表；`delete` 删除作者工具并遵从后端发布状态限制。团队范围只开放已发布内容，团队成员不因此获得编辑权。商城公开/定向分发及管理员操作不在本轮命令范围内。`created` 列表支持 `--limit/--offset`，`store` 支持 `--category`；查询与权限由后端处理。

### 在画布使用

```bash
virse tool schema <tool-id> --canvas <canvas-uuid> --org <org-uuid> --json
virse tool run <tool-id> --inputs inputs.json --canvas <canvas-uuid> --org <org-uuid> --wait --json
virse tool run <tool-id> --instruction '完成这个任务' --canvas <canvas-uuid> --org <org-uuid>
virse tool runs watch <run-id> --canvas <canvas-uuid> --org <org-uuid> --json
virse tool runs get <run-id> --canvas <canvas-uuid> --org <org-uuid> --json
virse tool runs cancel <run-id> --canvas <canvas-uuid> --org <org-uuid>
virse tool runs interrupt <run-id> --canvas <canvas-uuid> --org <org-uuid>
virse tool runs resume <run-id> --canvas <canvas-uuid> --org <org-uuid>
```

输入 JSON 的键是 Schema 中的字段 ID；资产字段使用 `canvas_element`、`asset` 或 `aesthetic` 来源对象，由后端校验、编译并固定引用。无表单工具使用 `--instruction`。`--inputs -` 从 stdin 读取，`--locale zh|en` 控制执行语言。

可用 `--version` 选择正式版本，或 `--version-id` 指定构建产物 ID，二者互斥；Release ID 不等于构建产物 ID。CLI 在读取 Schema 后固定执行产物，防止准备输入时线上升级导致版本错配。默认由后端创建工具节点，也支持 `--target-element` 或显式布局。

运行默认立即返回 `run_id`，`--wait` 才等待。独立观察走 Canvas WebSocket `canvas_run_updated`，在连接就绪及重连后补读快照，不定时轮询。JSON 观察每行一个 `run_snapshot`。Ctrl+C 只停止观察；`cancel`、`interrupt`、`resume` 分别按服务端允许的真实状态执行。`needs_attention` 不自动重投，产物位于 `result.outputs`。

工具运行支持 `--idempotency-key`：重复原命令与 key 复用已固定的版本、输入和目标。结果未知时不要换 key。工具列表管理或运行控制操作没有额外幂等回执时，不盲目自动重发；先读取当前状态再决定后续操作。

## 输出与退出码

默认输出格式化 JSON，`--json` 输出单行 JSON；`generation watch --json` 每行一个 `task_snapshot` 事件。stdout 只输出结果，进度和错误写 stderr；JSON 模式下错误为 `{"error":{...}}`，包含业务码、HTTP 状态和可用的请求 ID；等待错误附带 `task_id`，受理结果不确定时附带 `idempotency_key`。

| 退出码 | 含义 |
| --- | --- |
| 0 | 命令成功；普通查询成功读取失败任务也属于查询成功 |
| 1 | 其他业务、本地配置或文件错误 |
| 2 | 参数错误 |
| 3 / 4 | 认证 / 权限错误 |
| 5 | 网络、服务或观察连接不可用 |
| 6 | 等待的任务失败、部分成功或取消；资源批次存在失败项 |
| 7 | 提交/资源/分享操作结果不确定，或任务待核对 |
| 8 | 等待超时 |
| 130 | Ctrl+C 停止等待 |

## 独立使用 SDK

```rust,no_run
use virse_api::Client;

# async fn example() -> virse_api::Result<()> {
let client = Client::new("https://api.virse.ai", "virse_sk_example")?;
let identity = client.me().await?;
# Ok(())
# }
```

SDK 提供 `resources`、`resource`、`resource_summary`、`resource_download_info`、`create_folder`、`resource_trash` 和 `mutate_resources`，以及类型化查询、资源详情和逐项操作结果。通用 `AccessContext` 支持组织、资源或画布访问入口。路径解析、树展示、重试恢复和持久化归 CLI；SDK 不依赖命令参数。

SDK 的 `share_settings`、`set_share_rule`、`remove_share_rule`、`share_impact`、`clear_share_settings`、`resolve_share` 和 `claim_share` 封装现有权限协议；`ShareAddress` 统一解析分享地址。权限规则、影响、解析和认领结果均使用公开类型，不依赖 CLI。

SDK 的 `SubmitTask::media` 提供共用的图片/视频/3D 请求构造，`create_asset_element` 和 `CreateAssetElement` 提供已有资产的元素创建接口。所有 HTTP 协议及请求类型放在 SDK；模型参数文件读取和来源路径解析仍由 CLI 负责。

SDK 的 `submit` 是单次调用，幂等 key 由调用方传入；自动重试和跨进程记录属于 CLI。`watch` 返回可取消的异步快照流，丢弃流不取消任务。SDK 不保存文件，下载返回可流式读取的响应。

## 验证与当前边界

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
cargo test --workspace -j 2 -- --test-threads=2
```

本地测试通过模拟 HTTP/WebSocket 验证协议及 CLI 子进程行为，不访问业务数据库或模型供应商。真实联调需显式设置测试环境 `VIRSE_BACKEND_URL` 和测试 Token；不要让测试生成落到默认 production。

2026-10-28 统一资产接入：本地全仓 **133 项测试通过**，新增测试覆盖上传/替换与同 key 恢复、快捷方式权限入口、版本冲突、下载完整性、查询同步游标及 WS 补漏/重连/权限撤销。`cargo fmt --all --check` 和 Clippy `-D warnings` 通过。dev5 实际探测中，资产上下文、目录创建/读取可用，但统一上传返回 `503 mutation_unavailable`，changes 返回 `503 changes_not_enabled`；CLI 已验证上传结果未知时保留恢复记录并退出 `7`。成功上传、替换及实时订阅的 dev5 端到端验收仍待后端就绪，不将本地协议测试当作线上成功验收。

2026-10-27 已在 dev5 完成真实 API Token 联调：画布列表/元素读取、引用已有图片组合创建、Seedream 4 生成、任务观察（running → succeeded）、资产详情与下载、同 key 重放和已完成任务恢复均通过。只新增一个任务和一个元素，原引用元素保持不变。联调修复了画布读取传空 `expand` 被后端拒绝的问题，现使用协议要求的 `expand=none`，并补充回归测试。此记录不代表其他模型、视频/3D 或生产环境已验收。

同日 dev5 的资源管理联调通过：路径/ID 浏览、树形展示、递归搜索、创建文件夹、改名、移动、复制、回收站、恢复、资源详情与下载，以及改名/删除后的原命令幂等重放。写操作仅针对新建测试目录，测试目录最终移入回收站。访问范围变更确认、分页同名歧义、部分失败和不确定响应另有本地协议测试覆盖，不把这些模拟场景称为已在生产验证。

同日新增的画布资产能力也完成 dev5 联调：Wan 3.0 参考图生成 2 秒 480p 视频、Tripo H3.1 生成无贴图 GLB，均通过任务观察完成并下载有效原文件；已有图片、视频与 GLB 放置成功，GLB 放置和生成同 key 重放没有新增节点或任务。Word/PDF 放置已覆盖协议测试，该账号未找到现成文档资产，尚未做真实文档放置验收。测试结果保留在 “A testing Canvas”，不代表所有模型或生产环境均已验证。

2026-10-28 权限与分享接入已通过 dev5 联调：查看完整名单、影响预览、设置测试邮箱的待接受规则、移除、清空本层规则、获取及解析链接均通过。只操作新建空目录，链接权限始终为零，没有给真实协作者新增权限；测试目录最终移入回收站。跨账号邀请认领、只读成员、继承规则不可修改、预览失败和写入响应丢失通过协议测试覆盖，尚未做双账号真实验收。完整本地回归共 40 项通过。

目标 Backend 需要已部署 V2 生成接口、任务迁移及计费接管，并启用 `CANVAS_GENERATION_TASKS_ENABLED`。服务未启用时原样返回错误，不降级到旧入口重复生成。

本阶段不包含更新检测、兼容性协商、强制升级、系统钥匙串、自动更新、Creator 对话式制作 CLI、社区审核/管理员工具运营与审美训练调优命令。未来更新机制仍区分 API 契约版本、SDK 版本和 CLI 发布版本：协议处理归 SDK，提示和发行渠道处理归 CLI；本轮不发送兼容性请求或版本协商头。


### 本地工具包验收记录（2026-10-28）

后端配套 PR：Atlas-Of-Imagination/virse-systems-monorepo#615；需求：virse_workspace#116；产品说明 PR：virse_workspace#117。

dev5 已通过标准 labeldeploy 部署该后端，新增迁移在目标表结构上重复执行校验通过后自动应用。真实 API Token 完成了本地初始化/校验、首次发布 1.0.0、更新 1.0.1、历史源码拉取并重新发布 1.0.2、同内容无变化发布，以及原子团队范围发布和错误范围纠正。工具运行使用固定 1.0.0 产物，WebSocket 观察到成功，并在测试画布产生了指定文字节点。

真实联调使用同一个作者账号；跨作者源码隔离、团队普通成员权限、Creator 兼容与故障注入以隔离 PostgreSQL/协议测试验证，尚未完成双账号浏览器验收。后端 PR 尚待正式合入，其他环境须先迁移和部署配套接口。

## 常用生成快捷命令

```bash
virse generation text --model <text-model> --prompt '写一段说明' --reference <element-id> --wait
virse generation remove-bg --model <remove-bg-model> --source-element <image-element-id> --wait
virse generation upscale --model <upscale-model> --source-asset <image-asset-id> --scale 4 --wait
virse resource upload ./expanded-mask.png '/素材' --json
virse generation outpaint --model <outpaint-model> --mask-asset <uploaded-mask-asset-id> --prompt '延伸背景' --reference <source-element-id> --wait
```

这些命令复用 V2 占位/任务提交、幂等 key、观察与恢复，不新增轮询。默认建立新输出节点；`--target-element` 更新指定同类型节点。去背景/放大要求且只接受一个 `--source-element` 或 `--source-asset`，放大倍数仍由所选模型能力校验。文本可带 `--mode`，不把图像参数误传给文本接口。

扩图需要配套后端的蒙版资产引用支持。先准备好包含原图、扩展范围与蒙版的图片并上传，再传 `--mask-asset`，或用 `--mask-element` 引用已有画布图片；CLI 不生成画板草稿，也不要求用户取得桶路径。扩图支持现有 `--parameters` / `--parameters-file`、布局、引用和等待参数。蒙版身份及内容版本由服务端固定，后续替换或撤权不会静默改用新字节。

完整未实现范围见 [CLI 能力缺口](docs/CLI-GAPS.md)。

### 本轮常用能力验收

2026-10-28：dev5 已实测空间及角色列表、余额、模型目录/收藏与恢复、创建画布、内容投影、资源筛选、批量移动及同 key 重放、批量复制、资源预览与画布副本预览。模型收藏已恢复，临时测试目录已移入回收站。放置现有资产可能产生副本，后续访问使用放置响应里的新资产 ID。

生成快捷命令及扩图蒙版引用已做本地协议/权限/版本回归；本轮未调用收费模型供应商验收，不把请求构造或模拟结果写成真实生成成功。扩图先部署配套后端 PR，再使用新命令。

## CLI 本机编排与现有 HTTP 能力

已补齐多文件/目录导入及持久批次、资源置顶/收藏/排序/共享入口移除、画布内容另存、永久删除/分批清空、Canvas V2 元素/组合/连线管理与自动排版、节点复制与最近删除恢复、讨论评论、任务列表/失败输出重试、工具封面上传与待运行实例、ZIP交付和安全断点下载。

命令、JSON示例、恢复语义及明确边界见 [资源工作流](docs/CLI-WORKFLOWS.md)。这轮新增能力只使用现有后端；此前扩图快捷命令的配套后端依赖不变。

### CLI-only 工作流验收（2026-10-29）

dev5 已验证本机批次的准备/暂停/恢复及成功项不重传，资源标记与排序，节点/组合/连线修改与原子排版，复制及同 key 重放，讨论评论生命周期，媒体删除恢复，画布另存及重放，文件下载与ZIP，任务列表、封面上传、待运行工具实例及重放。测试工具已恢复禁用。SDK 已修正空 POST/DELETE 的 Content-Length，兼容网关的411检查。

空目录永久删除通过；包含画布和资产的测试目录在移入回收站后，永久删除连续返回后端 `503 mutation_unavailable`。目录仍列在回收站，原 key 和恢复记录保留；该环境问题需要单独排查后端，不声称复杂子树永久删除或完整清空已通过真实验收。本轮没有清空账号已有回收站、没有跨账号共享移除验收，也没有重试收费模型任务；这些分支用协议测试验证。

## 审美创作（现有能力）

`aesthetic` 使用 `/v2/aesthetics` 公共门面，保留现有训练、草稿和保存语义，不引入本地审美包或正式版本。

```bash
virse aesthetic call create --data '{"name":"柔和光线","image_ids":["<image-id>"],"organization_id":"<org-id>","defer_dentry":true}'
virse aesthetic train --canvas <canvas-id> --org <org-id> --idempotency-key <key> --data '{"name":"柔和光线","image_ids":["<image-id>"],"defer_dentry":true}'
virse aesthetic watch <run-id> --canvas <canvas-id> --org <org-id>
virse aesthetic call test-canvas --data '{"style_id":"<style-id>","org_id":"<org-id>"}'
virse aesthetic call start-draft --data '{"style_id":"<style-id>","canvas_id":"<test-canvas-id>","org_id":"<org-id>"}'
virse aesthetic call context --id <session-id>
virse aesthetic call revise --id <session-id> --data '{"expected_revision":0,"summary":"修改后的完整审美正文"}'
virse aesthetic call snapshot --id <session-id> --data '{"expected_revision":1}'
virse aesthetic call save-draft --id <session-id> --data '{"expected_revision":1}'
virse aesthetic call save-resource --data '{"style_id":"<style-id>","org_id":"<org-id>","drive":"personal"}'
```

`call` 的 JSON 沿用现有接口字段；编辑正式资源需要时传入已有资源访问上下文 `dentry_id`。`context` 仅供拥有编辑权限的调用者读取；普通详情仍隐藏内部正文。写入不自动重试；修订冲突应重新读取并审阅，不能静默覆盖。训练 watch 不轮询，Ctrl+C 不取消任务。训练取消只对 queued 生效。

本地 Agent 指南见 [skills/aesthetic-creator/SKILL.md](skills/aesthetic-creator/SKILL.md)，不会自动安装。
