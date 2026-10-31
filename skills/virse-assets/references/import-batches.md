# 导入批次

## 准备与执行

文件夹导入用于保留本地目录层级并持续记录逐项状态：

```bash
virse resource import ./materials --parent '/项目素材' --conflict rename --defer --org "$ORG" --json
virse batch get "$BATCH" --json
virse batch run "$BATCH" --org "$ORG" --json
```

不带 `--defer` 时创建批次后继续执行。批次冻结原始字节、父子关系和目标，运行时不重读已经变化的原件。后续用返回的 `batch_id`，不是文件项 ID。

支持 PNG/JPG/JPEG/WebP/GIF/BMP/AVIF、MP4/MOV/WebM/M4V、DOCX/PDF/GLB。每文件最多 100 MiB，总字节快照最多 2 GiB；不支持格式、符号链接与特殊项会记录跳过。项目或目录深度超限应拆分范围。目录 `skip` 跳过整棵同名子树，不合并目录；`rename` 的真实名称以结果为准。

## 查看本机进度

```bash
virse batch list --json
virse batch get "$BATCH" --json
```

指定批次报不存在时，错误码不一定准确；用 `batch list` 与当前配置目录的本地记录复核 ID 和上下文，不立即重新创建或上传。

这两个命令可离线读取当前配置目录的记录；不是跨机器统一任务服务。检查每项 `status/resource_id/result/error` 以及批次整体状态。执行会核对冻结的 backend、账号与组织；显式组织不匹配会拒绝，不通过改默认组织迁移批次。

## 暂停取消与接续

```bash
virse batch pause "$BATCH" --json
virse batch resume "$BATCH" --org "$ORG" --json
virse batch cancel "$BATCH" --json
virse batch retry "$BATCH" --item "$ITEM" --org "$ORG" --json
```

暂停和取消控制后续派发，不撤销在途请求或删除已成功资源。控制写入不代表执行者已到安全终点；再读取进度。`resume` 继续原队列，已成功项不重传。`retry` 只处理允许恢复的项；确实要重启取消项时明确加 `--include-cancelled`，不把成功项清零。

## 核对未确认文件或目录

目录创建没有幂等回执。出现 `unknown` 时先查目标父目录，再把已核实对象绑定回原项：

```bash
virse batch bind-folder "$BATCH" "$ITEM" "$FOLDER_RESOURCE" --org "$ORG" --json
virse batch bind-file "$BATCH" "$ITEM" "$FILE_RESOURCE" --org "$ORG" --json
```

绑定允许处理符合条件的 `unknown/failed/running` 遗留项。目录的 CLI 校验只核对 folder 类型和冻结父目录；**同父目录中的任意文件夹通过校验，不证明它就是原请求创建的目录**。应有原创建回执、能对应原请求的记录，或用户明确指定采用该目录的依据；无法确认时保留 unknown，不把其他文件夹绑定后接着派发子项。

`bind-folder` 后必须再查父项、子项、批次状态与 worker。已知可能出现父项 `succeeded`、子项仍 `blocked`、worker 为 false 而整体仍 `running`；绑定成功不等于队列可继续或批次完成。遇到这一组合，保留已绑定结果和未执行范围，停止派发并报告阻塞，不私自 `resume`、重传或换批次绕过。

文件必须匹配冻结父目录、大小与服务端 SHA-256。不能凭同名认定内容相同，也不能重复建目录来探测旧请求结果。没有成功快照的文件不靠重试重新读取新字节。

## 清理本机记录

用户要求移除已终结的批次恢复材料时：

```bash
virse batch forget "$BATCH" --yes --json
```

存在 `queued/blocked/running/unknown` 项时拒绝清理。成功后只删除该批次的本机快照和回执，不删除远端资源。保留成功、失败、跳过和未确认范围的区别，再报告最终完成情况。
