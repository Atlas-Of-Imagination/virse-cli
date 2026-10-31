# 观察、历史与停止

## 读取历史与对应回合

```bash
virse --org "$ORG" --canvas "$CANVAS" --json composer get "$COMPOSER"
virse --org "$ORG" --canvas "$CANVAS" --json composer history "$COMPOSER" --limit 20
virse --org "$ORG" --canvas "$CANVAS" --json composer history "$COMPOSER" --limit 20 --before "$CURSOR"
```

历史按 NDJSON 输出事件与分页元信息；在实际 history_meta 中读取 hasMore/nextCursor，保持同一节点续页。不要按消息数猜游标，也不要把历史回放当作所有临时事件的无损审计记录。

只查已完成回合时，优先核对保存状态与同一 run/turn 的历史终态，无需再连接实时观察器。节点最新回合不一定就是用户提到的那次未知提交；缺少原回执或输入对应依据时，说明能确认的是哪一轮。

## 观察当前或指定回合

```bash
virse --org "$ORG" --canvas "$CANVAS" --json composer watch "$COMPOSER" --timeout 600
virse --org "$ORG" --canvas "$CANVAS" --json composer watch "$COMPOSER" --turn "$TURN" --timeout 600
```

第一种观察节点当前运行，第二种固定 Agent turn；TURN 不是 run ID。没有当前运行且未指定 turn 时可能返回 idle，这不证明旧任务成功。

CLI 结合实时事件、保存状态和断线后的分页历史补读，不另外搭周期轮询或重新 send。输出可区分 composer_event 与 composer_history_event，不保证跨两类来源 exactly-once。观察恢复超出限制时，按 history 分页读取，不伪称完整恢复。

`watch` 即使观察已结束 turn，也先连接画布观察器。返回 `OBSERVATION_UNAVAILABLE` 时，用同节点 get 和分页 history 对账已有 run/turn；若保存状态与对应历史终态一致，可报告该轮终态，并说明实时观察未成功。没有对应证据就保留未知，不重新 send，也不把传输失败解释为远端任务失败。

真实 completed/turn_completed 对应成功退出 0；失败、取消或相应终态事件退出 6。turn_interrupted 只是中断受理或处理中，不能当作已取消终态。等待超时退出 8，Ctrl+C 退出 130，都不会取消平台执行。未知状态不套生成任务的 succeeded/partially_succeeded 解释。

## 请求停止并确认结果

```bash
virse --org "$ORG" --canvas "$CANVAS" --json composer stop "$COMPOSER" --wait --timeout 600
```

省略 wait 时只取得 interrupt_requested；需要确认停止才继续观察真实终态。stop 针对该节点当前运行，不能按客户端持有的旧 turn ID 原子绑定；先核对目标节点，避免将它解释成精准停止某个历史回合。

协作中断受理不代表供应商立即停止，也不回滚已经完成的画布修改。删除节点不是停止。结束后记录真实终态、已完成的具体产物及尚未完成部分。

## 检查平台实际交付

从对应回合记录与实际画布对象确认交付，必要时按[对象身份](shared/context/objects.md)定位，依[文件交付](shared/contracts/delivery.md)取得真实内容。外部 Agent 按用户目标检查产物；仅看到文字“已完成”不能宣称文件已保存、图片已生成或质量已通过。
