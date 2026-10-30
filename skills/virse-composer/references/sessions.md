# 独立节点与会话

## 查找与读取创作家

```bash
virse --org "$ORG" --canvas "$CANVAS" --json composer list --offset 0 --limit 100
virse --org "$ORG" --canvas "$CANVAS" --json composer get "$COMPOSER"
```

列表范围是当前画布可见的创作家，offset 分页每页 1–200；按实际结果与分页信息续读，不把一页当全集。已知节点直接 get，目标必须是 agent 元素。

节点属性与运行信息是当前保存状态。节点 UUID、run ID 和 Agent turn ID 各有用途；get 成功不证明会话任务成功，更不证明产物质量。

## 创建独立节点

```bash
virse --org "$ORG" --canvas "$CANVAS" --json composer create --name '设计助理' --idempotency-key "$CREATE_KEY"
```

记录返回节点身份。创建节点不发送任务；已有节点继续会话无需再 create。创建沿用画布元素事务与原键恢复，受理未知时保存原名称、范围和键先查证，不能无键重复创建多个创作家。

## 改名与删除节点

```bash
virse --org "$ORG" --canvas "$CANVAS" --json composer rename "$COMPOSER" '新名称' --idempotency-key "$RENAME_KEY"
virse --org "$ORG" --canvas "$CANVAS" --json composer delete "$COMPOSER" --idempotency-key "$DELETE_KEY"
```

按任务选用。改名只改节点名称，不创建或分叉会话，不修改执行目标。删除通过元素事务删除节点，不等于 stop，不承诺取消运行、清除全部历史或回滚画布修改。需要停止时使用[观察与停止](observe-and-stop.md)中的专用动作。

布局和连线属于画布结构合同，不通过任意 Composer 请求字段修改。没有会话 fork、REPL 或整轮回滚入口。
