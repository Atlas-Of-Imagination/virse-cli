# 保存与清理

## 保存选定草稿修订

用户要把已选择的调优结果写回源审美时：

```bash
virse aesthetic call save-draft --id <session-id> --data '{"expected_revision":1}' --json
```

body 必需最新 `expected_revision`，可选 `version` 为要保存的已有非负修订索引；省略时保存当前修订。服务端要求会话空闲、源审美仍可编辑且可用，源 fingerprint 没被别人改变。成功时将选定正文写回源审美并更新 `savedRevision`，不会新建工具式正式版本。

已有 pending 训练时，服务端按现有规则提交 pending 再写入所选正文。源内容变化报 `SOURCE_AESTHETIC_CHANGED` 时，重读比较并决定下一步；不能修改预期修订或重建会话来强压新源。

普通返回仍是会话 DTO，核对 `savedRevision` 与选定版本及源身份，不把当前 `revision` 与保存的旧版本混淆。`savedRevision` 和成功回执证明的是服务端报告的保存结果，不能单独证明已独立回读源审美完整私有正文；没有合法读取入口时保留该验证边界，不通过换身份或越权读取补证据。草稿保存不保证尚未落盘的源审美已经在目标目录。

## 将审美保存为目录资源

源审美已完成训练、用户指定位置后：

```bash
virse aesthetic call save-resource --data '{"style_id":"33333333-3333-4333-8333-333333333333","org_id":"22222222-2222-4222-8222-222222222222","drive":"personal"}' --json
```

必需 `style_id/org_id`；`drive` 默认 personal，可为 team；可选 `parent_id` 为目标文件夹 dentry、`name` 为显示名、`scope` 为分享意图。没有 parent 时是该组织资源根。不要把 canvas ID 当目录 parent。

新落盘要求调用者为草稿直接创建者、训练已 completed，目标位置可写。明确 `scope:org` 会向组织全体成员开放；未传 scope 时，`drive:team` 也表达该开放意图。其他合法 scope 不添加全体成员规则，访问仍可能受落点继承影响。仅在用户要求该范围时使用，普通保存不默认扩大访问，权限以当前后端结果为准。

已经存在正式 dentry 时，此动作要求该资源写权、非回收站状态，返回已有位置并按规则提交 pending；**不会把资源移动到本次新 parent，也不会靠新 name 改掉原落点**。要移动或整理现有资源，使用对应资源管理动作。

成功返回包含 `dentry` 和 `already_saved`，既有资源还可有 `pending_committed/pending_reason`。核对真实资源身份、落点和提交结果；不是只看 HTTP 或 exit 0。保存结果未知时核对同 style 的既有资源和状态，不换 style 另建副本掩盖不确定结果。

## 恢复会话内旧修订

```bash
virse aesthetic call restore --id <session-id> --data '{"expected_revision":2,"version":0}' --json
```

`expected_revision` 为当前修订，`version` 为已有修订索引且必需。会话须空闲；服务端把旧内容追加成一份**新修订**，不倒退修订号，50 份上限仍生效。restore 只改变草稿，不自动写回源审美，不回滚已经生成的测试文件；需要保留时重新测试并显式保存。

## 清理专用测试画布

确认不再需要该现场，且任务未要求保留对比材料后：

```bash
virse aesthetic call delete-test-canvas --id <test-canvas-id> --json
```

ID 为测试画布 UUID，body 无必需字段，默认 `{}`；只能删除调用者拥有且带专用标记的测试画布。不存在时可返回 `deleted:true`。仍有活动调优 run 时可能返回 `deleted:false, retained_for_run:true`，保留现场等待运行，不把它报告为已经删除。

实际删除会清理测试画布及其内部草稿/副本，不等于取消所有远端任务，也不撤销此前保存的正式资源。删除后原会话可能无法再使用；需要保存的内容应先按用户要求完成保存并核对。

最终交付分别说明训练 run 的结果、测试作品与快照、保存的源修订、目录 dentry 或未保存状态，以及保留/已删除的现场。需要实际文件时读取[交付合同](shared/contracts/delivery.md)。
