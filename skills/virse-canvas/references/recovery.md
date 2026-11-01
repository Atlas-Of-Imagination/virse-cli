# 媒体恢复

## 查看近期删除

```bash
virse canvas deleted list --org "$ORG" --canvas "$CANVAS" --json
```

读取该画布近期删除接口提供的媒体条目，先辨认目标及实际恢复身份。列表不是完整画布历史，也不是资源库 `resource trash`；不要拿活跃节点 ID、目录资源 ID 或自己推测的 ID 恢复。

## 恢复选定媒体

```bash
virse canvas deleted restore "$DELETED_MEDIA" --idempotency-key "$KEY" --org "$ORG" --canvas "$CANVAS" --json
```

`DELETED_MEDIA` 使用近期删除列表中目标媒体的实际恢复 ID。只恢复接口允许的媒体，不承诺恢复任意节点、组合、连线或历史版本。返回后核对恢复结果，再读取实际画布确认落点和内容。

结果不明时保留原画布、原 ID 与原请求键重放；不能选新的对象或用资源库恢复规则代替本接口。需要恢复资源目录中的文件时使用资产回收站职责，而非本命令。
