# Composer CLI

Composer 是画布中的 `agent` 元素。配置 API token、`VIRSE_BACKEND_URL`、组织与画布后使用：

```bash
virse composer create --name '设计助理' --idempotency-key create-assistant
virse composer list --offset 0 --limit 100
virse composer get <element-id>
virse composer rename <element-id> '新名称'
virse composer delete <element-id>
virse composer send <element-id> --input '根据参考图设计三个方案' --reference <image-element-id> --mode auto --wait
virse composer send <element-id> --input-file brief.md
cat brief.md | virse composer send <element-id> --input-file -
virse composer send <element-id> --request-file request.json
virse composer history <element-id> --limit 20
virse composer history <element-id> --limit 20 --before <nextCursor>
virse composer watch <element-id> --timeout 600
virse composer watch <element-id> --turn <agent-turn-id>
virse composer stop <element-id> --wait
```

所有命令沿用全局 `--org`、`--canvas`、`--json`。创建/改名/删除支持原幂等键恢复；布局、尺寸、连线使用现有 `canvas` 命令。删除节点不是停止命令。

## 消息及上下文

`send` 默认异步受理，返回后端 run_id 和 Agent agent_turn_id；两者不是同一个标识。`--wait` 在发送前连接画布 WebSocket，然后观察本轮。已有节点首轮会话初始化由后端负责。上一轮结束后继续向同一元素发送即可延续对话。

普通输入默认 auto，可显式 plan。`--reference` 只引用指定元素，不自动展开连线。复杂配置用请求 JSON，支持 input、mode、context、references、agent_model、allowed_models、allowed_aspect_ratios、allowed_resolutions、allowedStyles/allowed_styles、generation_settings、image_quality、language、organization_id；organization_id 若提供必须与 --org 一致。其他字段拒绝，不让服务端静默丢弃。JSON 文件与消息构造 flags 互斥。

## 输出、等待和恢复

- 单次查询为 JSON 对象，消息等待/历史/观察为 NDJSON。恢复的历史事件标记为 composer_history_event，可与实时 composer_event 区分；不保证所有临时事件无损回放或跨实时与历史 exactly-once。
- 消息提交没有幂等回执，只尝试一次。受理结果未知返回 WORKFLOW_UNCERTAIN（退出码7），记录 request ID，不自动重发。重复执行 send 是新的消息，可能重复执行及计费。
- 本地记录隔离账号、后端、组织、画布和节点，保存请求摘要及受理身份，不保存 token 或请求全文。
- 运行中再次发送会保留后端 agent_run_in_progress（409）。没有消息队列、运行中追加输入或精确续跑。
- watch 固定当前运行或指定 Agent turn。重连查询状态和分页历史，不做周期 HTTP 轮询。受理缺少 turn ID 时先按 run ID 对账，不从未知实时帧猜执行身份。
- 历史回放按页最多32MiB、单帧8MiB；观察恢复最多100页、5次断线重连。超过限制明确失败，可用 history 分页读取。
- Ctrl+C 退出观察（130），等待超时（8）不取消服务端执行；完成退出0，失败或取消退出6。未知状态不伪装为失败/成功。
- stop 对节点当前运行发送协作式中断；默认仅报告 interrupt_requested。--wait 等待真实终态；turn_interrupted 只是受理/处理中，turn_cancelled 才是取消。停止不能按客户端旧 turn ID 原子绑定，也不会撤销已完成的画布修改。

## 边界与验证

本轮只复用既有 V2 元素接口、/agent/blocks 消息/历史/interrupt 和画布 WebSocket；未修改后端或 Agent。未实现 REPL、fork、abort/rollback。

2026-10-29 dev5：独立 Composer 的创建/列出/改名、两轮对话、历史、独立 watch 均通过；第三轮运行中 stop --wait 收到 turn_interrupted 后收到 turn_cancelled 并退出6。测试对话要求不使用工具，未生成图片或视频。回放、短任务、执行隔离和丢响应路径另有本地协议测试，不代表所有 Agent 模型或生产环境已验收。
