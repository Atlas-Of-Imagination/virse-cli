# 回收与清理

## 移入回收站

```bash
virse resource delete "$RESOURCE" --idempotency-key "$KEY" --org "$ORG" --json
```

可提供多个资源，文件夹内容按服务端规则一同处理。读取逐项结果；部分失败不整批重做已成功项，未知结果使用原命令与原键核对。用户要求删除通常指可恢复删除，不自动升级为永久清理。

## 查看与恢复

```bash
virse resource trash --org "$ORG" --json
virse resource trash --scope "$SHARED_FOLDER_RESOURCE" --org "$ORG" --json
virse resource restore "$TRASH_RESOURCE" --idempotency-key "$KEY" --org "$ORG" --json
```

回收站只返回当前身份在所选范围有权查看的根项。`restore` 接受列表中的资源项 ID，可提供多个，不接受旧路径或 asset ID。恢复位置、重名处理和权限以实际回执为准，成功后读取最终资源位置。

资源库回收站与画布 `canvas deleted` 是不同对象与入口，不互换 ID，也不把任一列表当完整历史版本。

## 永久删除选定根项

仅在用户要求永久清理相应对象时：

```bash
virse resource purge "$TRASH_RESOURCE" --yes --idempotency-key "$KEY" --org "$ORG" --json
```

目标必须是回收站根项资源 ID，清理资格由后端判断，不以拥有内容编辑资格代替永久清理资格。结果逐项核对，永久删除不能恢复。

## 有界清空与恢复

用户要求清空有权处理的回收站时：

```bash
virse resource empty-trash --yes --max-batches 20 --idempotency-key "$KEY" --org "$ORG" --json
```

此命令分批清理服务端授权候选并保存页进度；达到 `max-batches` 不等于已清空。同键继续原操作，保留失败及未知范围，只有服务端返回空候选才能确认当前可处理范围已经耗尽。它不会为无权对象扩大权限。
