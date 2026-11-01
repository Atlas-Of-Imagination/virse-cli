# 资源变化

## 从现在开始观察

```bash
virse resource watch --org "$ORG" --json
virse resource watch --timeout 60 --org "$ORG" --json
```

不传游标会先建立“从现在开始”的基线，不输出初始资源树。JSON 为逐行事件页和通知：页含 `type=page/items/next_cursor/has_more/bootstrap`，重连通知为 `type=notice`。处理完一整页后再保存该页 `next_cursor`；通知不是新检查点。

WebSocket 只负责唤醒 HTTP 按游标补齐，在就绪、通知或重连时追到 `has_more=false`，没有周期轮询。CLI 不保存消费者缓存或游标，调用方需保存已经处理完整页的检查点。

## 续接已处理变化

```bash
virse resource watch --cursor "$CHANGES_CURSOR" --org "$ORG" --json
```

观察游标与原组织绑定。`--timeout` 是本机观察时长，正常到期退出 0；Ctrl+C 退出 130，都不表示取消某个远端业务任务。失败时保留最后实际输出并处理完整的页游标，不能改成新基线掩盖遗漏。

## 完整快照接续增量

需要建立资源本地视图时，先读取 `resource ls / --recursive --json` 的同一快照全部分页，保留相同查询范围和过滤条件，再把该快照实际返回的 `sync_cursor` 交给 `watch`。缺少同步水位时，不承诺无缝快照接续。

目录 `next_cursor` 只用于目录翻页；它不是变更游标。独立执行的多次 `tree` 不是全局快照，不能全量读取完毕后另取最新水位而跳过中间变化。

## 处理失效与权限变化

`scope_invalidated` 表示消费者需丢弃该组织缓存并重新读取。`cursor_expired/sync_reset_required` 需要重新建立基线和全量读取；权限撤销则停止。后端未开启 changes 或观察不可用时明确报告，不自行降级成另一套轮询器，也不推测隐藏资源的内容。
