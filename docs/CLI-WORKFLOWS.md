# 资源批次、画布管理与文件交付

这些能力只封装已存在的后端接口，或在 CLI 本机编排。没有新增后端队列、迁移、取消供应商任务或浏览器渲染。

## 本机持久上传批次

```bash
virse resource import ./a.png ./b.pdf ./models --parent '/素材' --defer
virse batch list
virse batch get <batch-id>
virse batch run <batch-id>
virse batch pause <batch-id>
virse batch resume <batch-id>
virse batch cancel <batch-id>
virse batch retry <batch-id>
virse batch retry <batch-id> --item <failed-item-id>
virse batch retry <batch-id> --include-cancelled
virse batch bind-folder <batch-id> <uncertain-folder-item-id> <actual-folder-resource-id>
virse batch bind-file <batch-id> <uncertain-file-item-id> <actual-file-resource-id>
virse batch forget <batch-id> --yes
```

`import` 支持多个普通文件或目录，递归保留目录层级，先把原始字节复制到私有状态目录，再开始提交；`--defer` 只准备，不提交。默认最多 1000 个条目（`--max-items` 上限10000），深度64，文件单项100MiB，每批快照2GiB；超过规模请拆分。目录包含的非支持格式、符号链接及特殊文件明确记录跳过原因。文件与目录最终名称/ID以回执为准。

批次固定 Backend、账号、组织、目标目录、内容校验和及逐文件 key，运行时不再读取原文件。切换默认组织不会改写旧批次；显式 `--org` 与批次不一致会拒绝。`batch list/get/pause/cancel/forget` 为本机操作，可离线使用；运行、恢复、重试及绑定目录需重新鉴权。执行在前台，关闭进程后记录仍在，通过同一批次恢复，不另起重复批次。

- 暂停/取消停止后续派发。在途 HTTP 请求可以完成，已成功项保持成功；中断传输的项标记为 unknown，不能宣称服务器已取消。
- 成功项从不重置。重试只重置选定失败/待恢复项，然后继续仍待执行的队列；已取消项只在显式 `--include-cancelled` 时重启。
- 文件未知结果使用原 key、固定字节和目标恢复。后端回执过期等情况下可显式 bind-file，但必须核对同一目标父目录、文件大小和服务端 SHA-256，不能仅凭同名认定成功。目录创建没有后端幂等合同，未知结果必须先查询实际目录并 `bind-folder`，绝不猜一个同名目录或再次创建。
- 目录同名策略为 `--conflict rename|skip|error`；skip 跳过整棵同名目录，不隐式合并。文件同名策略交给上传接口执行。目录名称检查不能承诺跨客户端并发唯一；始终使用返回的目录 ID。
- 原文件不可读、空文件等未成功快照的条目不能靠重试重新取新字节，需要修正材料后创建新批次。
- 每项分别记录 queued/running/succeeded/skipped/failed/blocked/unknown/cancelled；存在不确定结果退出7，存在失败或未完成项退出6。暂停/取消控制成功不代表全部文件上传成功。
- `forget` 只删除终结批次的本机快照和记录，不删除远端资源；有未提交、阻塞或未知结果时先处理这些项。

## 资源整理与回收站

```bash
virse resource pin '/素材/a.png'
virse resource unpin '/素材/a.png'
virse resource star '/素材/a.png'
virse resource unstar '/素材/a.png'
virse resource favorites
virse resource reorder '/素材' @<resource-id-2> @<resource-id-1>
virse resource hide-shared @<external-entry-id>
virse resource save-canvas --canvas <canvas-id> --element <element-id> --parent '/归档'
virse resource save-canvas --canvas <canvas-id> --group <group-id> --parent '/归档'
virse resource purge @<trash-root-id> --yes --idempotency-key purge-001
virse resource empty-trash --yes --max-batches 20 --idempotency-key empty-001
```

置顶和收藏分别写 pinned、quick_access；favorites 保留服务器返回的资源/工具入口类型。排序提交明确的成员顺序，权限与可见范围由服务端验证。移除外部共享入口不会撤权，也不会删除原件。

画布另存按元素逐项执行，组合不会合成一份文件。首次固定成员与目录 ID，逐项派生 key；重放沿用原请求，结果里保留成功、失败及 unknown。没有可另存文件的元素由服务器明确拒绝，不伪造成文件。

永久删除只接受回收站的资源 ID，并要求 `--yes`。清空每页最多200项，CLI保存页进度和稳定key，达到 `--max-batches`、出现失败或回执不明时停止。重新执行同一key继续核对；不会重新处理已完成页。只有服务端返回空候选页才标记本次操作完成；不声称以后不会出现新删除项，也不清理无权处理的范围。遇到失败后显式续接会重新查询剩余候选。物理文件回收仍由后端控制。

## 画布元素、组合、连线

所有写操作要求已部署 Canvas V2 及其版本机制。`--canvas` 是画布 Space ID，`--org` 是操作空间。首次写入读取目标的版本；也可用 `--revisions '{"layout":2}'` 明确提供先前读取的版本。冲突不会自动刷新版本强行覆盖，同key恢复保持第一次的目标与版本。

```bash
virse canvas element list --offset 0 --limit 100
virse canvas element get <element-id>
virse canvas element create --file element.json --idempotency-key create-001
virse canvas element update <element-id> --file patch.json
virse canvas element name <element-id> '新名称'
virse canvas element delete <element-id>
virse canvas group create --file group.json
virse canvas group update <group-id> --file group-patch.json
virse canvas group name <group-id> '新组名'
virse canvas group delete <group-id>
virse canvas edge create --file edge.json
virse canvas edge update <edge-id> --file edge-patch.json
virse canvas edge delete <edge-id>
virse canvas transaction --file transaction.json --idempotency-key transaction-001
virse canvas layout --element <id-1> --element <id-2> --mode grid --columns 2 --gap 40
virse canvas layout --group <group-id> --mode row --x 100 --y 100
virse canvas copy --group <group-id> --to-canvas <target-canvas-id> --dx 80 --dy 80
virse canvas deleted list
virse canvas deleted restore <deletion-record-id> --idempotency-key restore-001
```

元素创建文件示例：

```json
{"type":"text","name":"说明","text":"正文","layout":{"position":{"x":10,"y":20},"size":{"width":400,"height":200}}}
```

更新文件只写需要修改的字段，例如 `{"layout":{"position":{"x":40,"y":50}},"text":"修改后的正文"}`。不要把读取响应里的ID、canvas_id、revisions原样塞进patch。组合创建使用 `{"elements":["UUID1","UUID2"],"title":"一组","position":{"x":0,"y":0},"size":{"width":800,"height":400}}`；连线使用 `{"source_element_id":"UUID1","target_element_id":"UUID2","input_slot":"reference"}`，也支持后端允许的 source_group_id / target_group_id。具体节点类型与字段仍由后端校验。

事务 JSON 为 `{"org_id":"...","operations":[{"action":"element.update","id":"...","value":{"text":"..."},"expected_revisions":{"content":1}}]}`，最多100个操作。普通列表保留分页，不自动假装完整画布。

排版只调整显示位置，不修改媒体像素；支持 row/column/grid，按输入顺序计算，整个布局通过一个事务提交。最多100个元素；指定组合时最多99个成员并同时更新容器边界。复制固定节点快照与新ID、重映射内部连线和生成引用；服务端处理资产权限及会话Fork。可编辑MarkSpace文档不通过旧paste协议复制，避免丢失草稿；先导出已保存图片或使用后续专用编辑能力。删除组合遵循后端容器删除语义；删除节点不等于取消生成。最近删除仅是画布媒体恢复，不是空间回收站，也不是全局undo/redo。

## 讨论与回复

```bash
virse canvas discussion list --status open
virse canvas discussion get <discussion-id>
virse canvas discussion create --content '请检查这里' --element <element-id>
virse canvas discussion reply <discussion-id> --content '已检查'
virse canvas discussion edit <discussion-id> <comment-id> --content '更新说明'
virse canvas discussion resolve <discussion-id>
virse canvas discussion resolve <discussion-id> --reopen
virse canvas discussion read <discussion-id>
virse canvas discussion anchor <discussion-id> --detach
virse canvas discussion move <discussion-id> --x 100 --y 200
virse canvas discussion anchor <discussion-id> --group <group-id>
virse canvas discussion delete <discussion-id> --comment <reply-id>
virse canvas discussion delete <discussion-id>
```

评论1..200字符，位置与锚点变更沿用作者/编辑者权限。需要移动已有绑定标记时先解绑；`anchor:null` 表示显式解绑。评论写入没有持久幂等回执，CLI只发送一次；结果不明退出7，先读取讨论确认，不自动重复发言。

## 任务与工具

```bash
virse generation list --status failed --element <element-id> --offset 0 --limit 50
virse generation list --artifact-version <version-id>
virse generation retry <task-id> --idempotency-key retry-001 --wait
virse tool cover upload ./cover.png
virse tool place <tool-id> --version 1.0.0 --name '待运行工具' --idempotency-key place-001
```

任务列表按画布读取，可按任务ID、元素、产物版本、状态和操作筛选；满页不等于一定还有下一页，继续读取时保持条件。retry由服务端选择允许重试的失败输出，返回关联的新任务；不重新生成成功输出，也不改写原任务。

封面上传单图最多10MiB，返回 coverUrl；把URL写入本地meta.json并publish才更新工具资料。单独上传不修改工具定义，回执未知时不自动重发。`tool place` 先固定正式产物版本，再创建实例，返回 started=false；需要执行时用 `tool run --target-element` 明确启动。

## ZIP 与断点下载

```bash
virse resource pack '/素材/a.png' '/素材/b.pdf' --output deliver.zip
virse canvas pack --element <element-id> --element <another-id> --output selected.zip
virse canvas pack --group <group-id> --output group.zip
virse resource download '/素材/video.mp4' --output video.mp4 --resume
virse asset download <asset-id> --output model.glb --resume
```

ZIP中只放真实下载成功的文件和 manifest.json；文件名消除路径穿越并加序号，逐文件记录来源、内容版本、大小和错误，不保存临时授权URL。普通资源打包最多200项，画布组合最多200个成员。文本等非文件节点明确标记 skipped，不伪造图片。部分下载失败仍交付可用ZIP并退出6；没有任何成功文件也退出6。

断点下载在输出旁保存私有 `.virse-part`、`.virse-part.json` 与锁文件。恢复前检查账号/环境/访问上下文、内容版本与已下载前缀校验和，丢弃未检查点化的尾部。206核对Content-Range；服务端返回200则重写整份，绝不直接追加。最终校验大小和可用SHA-256，通过后才发布目标文件；已有目标文件不覆盖。内容已变化时保留旧部分并明确报错，选择新输出路径重下。没有可验证对象版本、校验和或强ETag的来源不承诺安全续传。
