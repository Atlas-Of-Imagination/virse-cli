# Virse CLI

Rust 命令行客户端，面向交互使用、脚本和 Agent。已经实现 API Token 认证、按路径浏览和管理资源、画布读取、已有资产放置、图片/视频/3D 组合创建与生成、WebSocket 观察，以及文件下载。

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

### 创建、改名、移动、复制

```bash
virse resource mkdir '/素材'
virse resource mkdir '/素材/产品图'
virse resource mkdir '新文件夹' --parent @<folder-uuid>
virse resource rename '/素材/产品图' '产品摄影'
virse resource move '/素材/产品摄影' '/归档'
virse resource copy '/归档/产品摄影' '/素材'
```

`mkdir` 一次创建一个文件夹，父目录必须存在；同名项已存在时拒绝创建。`rename` 的第二个参数是新名称，`move` / `copy` 的第二个参数是已存在的目标文件夹或 `/`，不把它解释成新文件名，不隐式覆盖或合并资源。

移动可能改变哪些人能访问资源。后端要求确认时，CLI 返回变化详情并停止；核对后可显式使用 `--confirm-access-change` 重发。确认后的操作使用新幂等 key，不自动接受权限变化。

### 回收站与恢复

```bash
virse resource delete '/素材/旧图.jpg'
virse resource delete @<resource-uuid-1> @<resource-uuid-2>
virse resource trash --json
virse resource trash --scope '/共享文件夹' --json
virse resource restore @<resource-uuid>
```

`delete`（别名 `rm`）将资源移入回收站；文件夹连同内容处理，与 Web 行为一致。`restore` 使用回收站返回的 ID，恢复位置和访问权限以服务端结果为准。当前不提供彻底删除和清空回收站命令。回收站列表沿用 Web 的可见范围，不能用它证明所有历史删除记录均已列出。

### 失败与重试

改名、移动、复制、删除和恢复均使用后端统一幂等接口，支持 `--idempotency-key`。CLI 在发送前冻结账号、组织、已解析的资源与目标目录；自动重试以及同 key 手动恢复使用这一份请求。因此即使原路径已经改名或删除，同一条命令重放也不会重新选中其他同名资源。

```bash
virse resource rename '/素材/a.jpg' 'b.jpg' --idempotency-key rename-001 --json
# 响应丢失时，重复原命令和 key；不要将源路径改成新名称。
virse resource rename '/素材/a.jpg' 'b.jpg' --idempotency-key rename-001 --json
```

恢复记录保存在当前配置目录，按 Backend、账号、组织和 key 隔离，含资源 ID、操作参数与指纹，不含 Token。不要在请求结果未知时删除记录。换设备后如没有原记录，应使用原资源 ID 确认状态，不能猜测旧路径对应的资源。

批量结果逐项报告；任一项失败时返回退出码 `6`，不重复执行已成功项。确定失败项需要重新执行时，只提交这些项并使用新 key。网络错误或响应损坏后的最多三次自动重试始终使用原请求；结果仍未知时返回退出码 `7` 和 key。

文件夹创建目前没有后端幂等保障，因此不自动重试。响应丢失时先列父目录确认，CLI 会明确报告“创建结果待确认”。上传/导入留待下一轮实现。

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

2026-10-27 已在 dev5 完成真实 API Token 联调：画布列表/元素读取、引用已有图片组合创建、Seedream 4 生成、任务观察（running → succeeded）、资产详情与下载、同 key 重放和已完成任务恢复均通过。只新增一个任务和一个元素，原引用元素保持不变。联调修复了画布读取传空 `expand` 被后端拒绝的问题，现使用协议要求的 `expand=none`，并补充回归测试。此记录不代表其他模型、视频/3D 或生产环境已验收。

同日 dev5 的资源管理联调通过：路径/ID 浏览、树形展示、递归搜索、创建文件夹、改名、移动、复制、回收站、恢复、资源详情与下载，以及改名/删除后的原命令幂等重放。写操作仅针对新建测试目录，测试目录最终移入回收站。访问范围变更确认、分页同名歧义、部分失败和不确定响应另有本地协议测试覆盖，不把这些模拟场景称为已在生产验证。

同日新增的画布资产能力也完成 dev5 联调：Wan 3.0 参考图生成 2 秒 480p 视频、Tripo H3.1 生成无贴图 GLB，均通过任务观察完成并下载有效原文件；已有图片、视频与 GLB 放置成功，GLB 放置和生成同 key 重放没有新增节点或任务。Word/PDF 放置已覆盖协议测试，该账号未找到现成文档资产，尚未做真实文档放置验收。测试结果保留在 “A testing Canvas”，不代表所有模型或生产环境均已验证。

2026-10-28 权限与分享接入已通过 dev5 联调：查看完整名单、影响预览、设置测试邮箱的待接受规则、移除、清空本层规则、获取及解析链接均通过。只操作新建空目录，链接权限始终为零，没有给真实协作者新增权限；测试目录最终移入回收站。跨账号邀请认领、只读成员、继承规则不可修改、预览失败和写入响应丢失通过协议测试覆盖，尚未做双账号真实验收。完整本地回归共 40 项通过。

目标 Backend 需要已部署 V2 生成接口、任务迁移及计费接管，并启用 `CANVAS_GENERATION_TASKS_ENABLED`。服务未启用时原样返回错误，不降级到旧入口重复生成。

本阶段不包含更新检测、兼容性协商、强制升级、系统钥匙串、自动更新、空白/非资产元素创建与编辑、工具与审美命令。未来更新机制仍区分 API 契约版本、SDK 版本和 CLI 发布版本：协议处理归 SDK，提示和发行渠道处理归 CLI；本轮不发送兼容性请求或版本协商头。
