# 创建与训练

## 创建或更新审美

从用户选定、有权读取的图片资产建立审美。示例 UUID 均需替换为当前真实身份；JSON 是 HTTP 参数，不是可导入的审美包。

```bash
virse aesthetic call create --data '{"name":"柔和侧光","image_ids":["11111111-1111-4111-8111-111111111111"],"organization_id":"22222222-2222-4222-8222-222222222222","defer_dentry":true}' --json
```

新建省略 `style_id`，提供有意义的 `name` 和选定 `image_ids`；调用者成为创建者。`organization_id` 可选，省略时由原创建服务关联个人组织；任务已有组织时显式传入，不自行跨空间创建。`defer_dentry:true` 先创建并训练草稿、不自动入资源根目录，训练后按用户位置要求显式保存；省略时沿用创建即入树的旧行为，二者不能混淆。

可选 `description` 是用户可见描述，`prompt` 是最多 4000 字符的风格指导词，语义不同。传已有 `style_id` 时更新展示信息、名称或指导词；`image_ids/member_ids/owner_ids/space_ids` 仅创建分支生效，不能用更新 `create` 换参考图。修改正式资源时在 body 中带匹配的 `dentry_id`，并按需提供 `organization_id`；没有正式资源身份的草稿只认创建者。

```bash
virse aesthetic call create --data '{"style_id":"33333333-3333-4333-8333-333333333333","name":"柔和侧光","description":"适合安静自然的商品展示","dentry_id":"44444444-4444-4444-8444-444444444444"}' --json
```

回执包含 `status`、`message` 与裁剪后的 `style`；记录 `style.id` 和真实进度。有图片时创建会触发异步训练，返回成功不等于训练完成；无图创建显示 completed 也不证明已有可调优正文或视觉效果。该创建入口没有训练 run 幂等回执，响应未知先查证，不能换名称重新创建。需要可跟踪 run 的训练使用下面的训练入口。

创建、替换参考集或训练若未返回可归因的单次 CU，记录“实际消耗未取得”，不写成 0；当前 CLI 没有按审美或训练 run 查询单次用量的入口，不能用共享团队余额差推算本次消耗。

## 替换参考图

`references` 是**整集替换并触发相应重训**，不是读取参考图。只保留最终应存在的集合：

```bash
virse aesthetic call references --data '{"style_id":"33333333-3333-4333-8333-333333333333","image_ids":["11111111-1111-4111-8111-111111111111"],"org_id":"22222222-2222-4222-8222-222222222222","dentry_id":"44444444-4444-4444-8444-444444444444"}' --json
```

body 仅接受 `style_id`、`image_ids` 和可选 `org_id/dentry_id`，不要传 `operation`；门面固定执行 replace。空数组代表清空，不代表“不改”。源审美须可编辑，新加入图片须可读；既有参考图可能被收口成审美内部资产，不凭原图 ID 假定保存后的集合身份完全不变。

正式资源的参考图变更进入 pending 训练，保存前不替换正式内容；未保存草稿可直接改变自身参考关系。成功回执的 `update_status` 只说明本次修改已接受，不证明重训完成或 pending 已提交。若要观察完整训练 run，优先使用下面的训练入口一次完成参考集合替换与训练，不先 references 再重复提交相同重训。

## 提交可观察的训练

```bash
virse aesthetic train --canvas <canvas-id> --org <org-id> --idempotency-key aesthetic-train-example --data '{"name":"柔和侧光","image_ids":["11111111-1111-4111-8111-111111111111"],"defer_dentry":true}' --json
```

`--data` 是内层 `TrainingInput`；CLI 自动把全局组织放在外层 `org_id`，并用画布建立训练上下文，不手工再套 `request`。内层沿用创建字段，但 `image_ids` **必需且为 1–100 个图片 UUID**，未知字段被拒绝。不能提交 `status/owner_ids/member_ids/space_ids` 等非空治理字段。内层 `organization_id` 省略或与外层组织相同；训练服务将其与 `space_id` 固定为当前组织和画布。

新建训练省略 `style_id`；已有审美重训在内层加 `style_id`，正式资源还须匹配 `dentry_id`。调用者须可写目标画布、可编辑源审美、可读参考图片；已有训练未完成或同源仍有活动训练时服务端拒绝。重训会替换参考集合，不能当作单纯改名。

保留原 `--data` 的完整对象、原组织、画布、幂等键和回执 run ID。键不为空、最长 200 字符；同一键只能重放完全相同的请求。**CLI 不为该入口保存本地全请求快照**，不能依赖后来记忆恢复；不确定受理时沿用原请求与原键核对，不换键新建可能收费的任务。

训练回执是 `kind=aesthetic_training` 的 run，初始可能 queued。记录 `run_id`，后续快照中的 `result.style_id`、`training_revision` 与进度作为该轮身份；受理、训练完成和资源保存分别确认。

## 查询与观察训练

```bash
virse aesthetic get <run-id> --canvas <canvas-id> --org <org-id> --json
virse aesthetic watch <run-id> --canvas <canvas-id> --org <org-id> --json
```

get 返回真实 run 快照；watch 在画布 WebSocket 就绪后补读并持续观察，输出裸 run 对象，不是 generation 的 `task_snapshot` 信封。读取 `status/revision/result/error/actions`。`succeeded` 表示该轮训练成功，仍需检查效果及保存状态；`needs_attention` 表示需要查证结果，不能自动重训。

watch 超时或 Ctrl+C 只停止本地观察，不取消远端训练。需继续时读取原 run；不自行实现重复提交或额外轮询代替 CLI 观察。查询命令 exit 0 只代表查询成功，run 可能失败。

## 取消与恢复训练

```bash
virse aesthetic cancel <run-id> --canvas <canvas-id> --org <org-id> --json
virse aesthetic resume <run-id> --canvas <canvas-id> --org <org-id> --json
```

先核对最新 `actions` 和真实状态；queued 仍须 `actions.cancel=true` 或 `actions.resume=true` 才执行对应动作。可操作窗口可能很短，调用前状态变化时按实际结果处理；没有取得合法样本只说明未验证该分支，不代表功能失败，也不拿 running、succeeded 或 cancelled 冒充可恢复对象。cancel 只对未投递的 queued 训练生效，已取消可返回同一状态；不能中断正在运行的训练。resume 只重新投递服务端仍允许的 queued run，不是任意失败断点恢复；该入口要求原训练拥有者及画布写权限。检查返回状态，不把动作受理说成已停止或已完成，不承诺退款或回滚内容。

训练完成后，要比较具体效果读[草稿与测试](draft-and-test.md)；要提交 pending 或存入资源目录读[保存与清理](save-and-cleanup.md)。
