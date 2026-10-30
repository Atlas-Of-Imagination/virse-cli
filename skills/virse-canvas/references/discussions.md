# 画布讨论

## 查看讨论

```bash
virse canvas discussion list --status open --canvas "$CANVAS" --json
virse canvas discussion get "$DISCUSSION" --canvas "$CANVAS" --json
```

列表支持 all、open、resolved。详情读取不会自动标记已读；要改变已读状态使用 `discussion read`。讨论均在指定画布空间下处理，权限由后端判断。

## 创建与回复

```bash
virse canvas discussion create --content '确认主图方案' --x 100 --y 200 --element "$ELEMENT" --canvas "$CANVAS" --json
virse canvas discussion reply "$DISCUSSION" --content '采用第二张' --canvas "$CANVAS" --json
virse canvas discussion edit "$DISCUSSION" "$COMMENT" --content '采用第二张并保留原比例' --canvas "$CANVAS" --json
```

内容去除首尾空白后须为 1–200 字。创建时可不设锚点，或在 `--element/--group` 中选择一种；坐标为有限值；有锚点时实际位置由锚定对象决定，不能用传入的 x/y 承诺自由定位，创建后读回核对。编辑针对指定评论，不能把讨论 ID 当评论 ID。

## 位置与锚点

```bash
virse canvas discussion move "$DISCUSSION" --x 300 --y 200 --canvas "$CANVAS" --json
virse canvas discussion anchor "$DISCUSSION" --group "$GROUP" --canvas "$CANVAS" --json
virse canvas discussion anchor "$DISCUSSION" --detach --canvas "$CANVAS" --json
```

`move` 仅适用于无锚点讨论；带锚点移动会被 `DISCUSSION_ANCHORED` 拒绝，用户要求保留锚点时不能自行解除来实现坐标。节点和组合锚点互斥，绑定后位置会落到关联对象右下角，读回实际坐标。

更换已有锚点须先 `anchor --detach`，确认解除后再绑定新对象；这不是原子替换，中间存在无锚点状态。第二步失败时如实保留并说明当前状态，不能把解除成功报成换锚点完成。解除保留当前坐标、讨论和原对象；以 `anchor_type/id` 为空核对，不把它等同 `anchor_deleted=true`。

## 解决重开与已读

```bash
virse canvas discussion resolve "$DISCUSSION" --canvas "$CANVAS" --json
virse canvas discussion resolve "$DISCUSSION" --reopen --canvas "$CANVAS" --json
virse canvas discussion read "$DISCUSSION" --canvas "$CANVAS" --json
```

解决设 `resolved=true`，重开设为 false，不删除讨论。已读更新服从当前身份的后端语义，不凭读取成功宣称已经标记。

## 删除回复或整个讨论

```bash
virse canvas discussion delete "$DISCUSSION" --comment "$COMMENT" --canvas "$CANVAS" --json
virse canvas discussion delete "$DISCUSSION" --canvas "$CANVAS" --json
```

指定 `--comment` 是请求删除相应评论，省略则删除整个讨论。只删普通回复时先辨明回复身份；首条评论的删除范围不能按普通回复推断。编辑与删除权限以服务端判定为准。

讨论写入没有持久幂等回执。响应不明时先读取讨论和评论确认，不能自动重发创建或回复；已确认写入后核对内容、锚点、位置和 resolved 状态。
