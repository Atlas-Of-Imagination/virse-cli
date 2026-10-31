# 草稿与测试

## 建立私有测试画布

源审美已训练完成且具备正文，用户希望调优时，创建或接续服务端专用测试现场：

```bash
virse aesthetic call test-canvas --data '{"style_id":"33333333-3333-4333-8333-333333333333","org_id":"22222222-2222-4222-8222-222222222222","dentry_id":"44444444-4444-4444-8444-444444444444"}' --json
```

`style_id/org_id` 为必需 UUID，正式资源提供可选字段 `dentry_id`；草稿可省略。调用者须有源审美编辑权且是组织成员。返回 `canvasId`，可能带 `resumed:true` 表示复用保留的现场。它是调用者直接拥有的 private 隐藏测试画布，不创建正式资源目录节点；不要拿任意可访问或共享画布代替。

## 创建或读取草稿会话

```bash
virse aesthetic call start-draft --data '{"style_id":"33333333-3333-4333-8333-333333333333","canvas_id":"55555555-5555-4555-8555-555555555555","org_id":"22222222-2222-4222-8222-222222222222","dentry_id":"44444444-4444-4444-8444-444444444444"}' --json
virse aesthetic call draft --id <session-id> --json
```

start-draft 必需 `style_id/canvas_id`，可选 `org_id/dentry_id`。源审美须可编辑、progress completed 且已有正文；pending 训练尚未完成时拒绝。画布须是当前用户直接拥有的专用 private 测试画布。同用户、源审美与测试画布已有未关闭会话时返回该会话，不能把它当成新建了另一份草稿。

返回的普通会话字段为 `sessionId`、草稿 `styleId`、`revision`、`savedRevision`、`versions` 和 `running`。记录源 style ID 与返回的草稿 style ID；二者不同。`versions` 是可选的会话修订索引，不是正式发布版本。

draft 是 GET，不发送 `--data`。每次读取都重新检查会话用户、源审美编辑权和测试画布归属；404 可能是会话不属当前用户、已关闭或已不可用，不能切换身份猜读。

## 读取作者上下文

```bash
virse aesthetic call context --id <session-id> --json
```

context 同为无 body 的 GET，返回 `session_id`、`canvas_id`、草稿 `style_id`、当前 `revision`、私有完整 `aesthetic_summary` 与 `reference_asset_ids`。它只用于已获准的作者工作；普通审美选择和详情读取不需要该接口。

根据参考图和用户目标明确保留项、具体问题与本次改动。完整正文不等于用户展示描述，不把它写进公共详情、资源说明或分享结果。需要查看真实参考或产物时读取当前可访问媒体，不凭文字猜效果。

## 修订完整正文

```bash
virse aesthetic call revise --id <session-id> --data '{"expected_revision":0,"summary":"柔和的侧向自然光，保留主体材质和真实色彩；背景低对比，避免强硬轮廓光。"}' --json
```

body 只接受 `expected_revision` 与 `summary`，前者为最新读取的非负整数，后者是**完整替换正文**，非空且最多 24000 字符；不得只传差异、模型对话或 `running` 字段。公共作者接口允许外部 Agent 修改空闲草稿，不要求启动内置 Agent 或伪造 begin。会话最多保留 50 份修订，包括初始内容；修订返回新的会话 revision。

修改仅作用于草稿。并发写按行锁与 `expected_revision` 检查；出现 `AESTHETIC_REVISION_CONFLICT` 时重新读取、比较并决定合并，不只替换数字覆盖他人改动。源正文已被别人修改时，保存还会单独检查源 fingerprint。

## 固定测试快照

确定要验证哪次正文时，在会话空闲且修订仍匹配时：

```bash
virse aesthetic call snapshot --id <session-id> --data '{"expected_revision":1}' --json
```

可加 `"version":0` 固定某个已有修订；省略时用当前修订。返回 `styleId` 和该目标 `revision`，它们标识冻结副本；每次显式 snapshot 都可能创建新副本，没有生成图片的副作用，也不增加正式 release。请求结果未知时先核对现场与原响应，不盲目重复制造快照。

回执中的快照身份与修订、操作前草稿正文是可记录的对应证据；它们不等于已独立回读冻结副本正文。普通详情可能拒绝未放置的私有快照，不据此断言不存在或创建失败。没有合法只读入口时说明冻结正文尚未独立核对，不越权读取，也不为补证据额外放置、生成或保存。

## 用真实产物比较效果

保存快照 style ID、会话修订、具体测试输入、模型/operation 和生成结果之间的对应关系。按[生成合同](shared/contracts/generation.md)查询当前支持审美的 operation，使用其真实字段绑定快照，保持可比较的主体、提示词和必要参数；不假定所有模型支持审美。

调用生成只说明测试已提交。继续观察原 task，逐输出读取，按[文件交付](shared/contracts/delivery.md)取得获准预览或原件并查看真实图片或其他作品，判断本次具体问题是否改善。快照固定的是审美内容，不保证相同随机结果；不要把不同快照、不同测试输入的效果混成单一结论。

效果不合适时重新读当前 revision、修订完整正文、固定新快照并测试。用户要保留选定结果时读[保存与清理](save-and-cleanup.md)，不把测试成功说成源审美已经保存。
