# 消息、上下文与继续会话

## 交办带明确材料的任务

已有节点上直接发送：

```bash
virse --org "$ORG" --canvas "$CANVAS" --json composer send "$COMPOSER" --input "$BRIEF" --reference "$ELEMENT" --mode auto --wait
```

引用多少个节点由任务决定，逐个 `--reference` 只引用指定元素，不自动展开相连节点或整个画布。说明目标、保留项和交付标准；不把外部 Agent 未确认的猜测包装成用户要求。运行中外部 Agent 不重复执行同一目标，但仍负责检查交付。

Composer 不自动继承宿主对话、本地文件或其他应用内容。把完成子任务必需且获准传入的事实、约束与材料写入 brief，需要画布素材时附真实节点引用；`--input-file` 读取消息正文，不代表正文提到的本地文件已经上传或可被 Composer 读取。

普通输入默认 auto，显式 mode 只支持 auto/plan；具体执行由平台处理，不能依据模式名推断额外权限或固定成本。

## 文件输入与复杂请求

```bash
virse --org "$ORG" --canvas "$CANVAS" --json composer send "$COMPOSER" --input-file brief.md
virse --org "$ORG" --canvas "$CANVAS" --json composer send "$COMPOSER" --input-file -
virse --org "$ORG" --canvas "$CANVAS" --json composer send "$COMPOSER" --request-file request.json
```

第二种从 stdin 读 UTF-8 消息；普通消息文件不超过 1 MiB。`input`、`input-file`、`request-file` 互斥，request-file 也不与 mode/reference 构造 flags 混用。

请求 JSON 必须有非空 input。受支持的顶层字段为 input、mode、context、references、agent_model、allowed_models、allowed_aspect_ratios、allowed_resolutions、allowedStyles/allowed_styles、organization_id、image_quality、language、generation_settings；未知字段会拒绝。可选嵌套内容按已核清的合同填写，不从字段名猜结构。organization_id 若提供必须与本次 org 一致，CLI 会设置当前组织。

模型、审美或其他受限对象的绑定只使用平台真实允许的请求字段与资格；不能把审美作者 context 或任意私有正文自动塞入普通上下文。

## 原会话继续与未知受理

上一回合结束后，先按需要读[历史与状态](observe-and-stop.md)，再向同一 COMPOSER 节点发送新的追问即可延续。新的 send 是新消息；不创建新节点，也不声称从上次任意内部断点继续。

send 只有一次提交尝试，没有可重放的幂等回执。`WORKFLOW_UNCERTAIN` 时保留 request ID 与原节点，先 get/history/watch 查证，不能自动重发。CLI 本地记录保存请求摘要和受理身份，不保存完整原文；原 brief 应自行保留。

受理后分别记录后端 run_id 和 Agent agent_turn_id。正在执行时再次发送可能返回 agent_run_in_progress；没有追加消息队列、运行中注入输入或精确续跑合同。明确需要新任务时按当前节点状态与用户目标处理，不把网络重试当作继续会话。
