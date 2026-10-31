# 工具运行合同

## 固定执行产物

tool ID 标识工具，正式版本标签可经 Schema 解析为不可变 artifact；artifact ID 与 release ID 不同。画布实例是节点，真正执行是 run，四种身份分别记录。

```bash
virse --org "$ORG" --canvas "$CANVAS" --json tool schema "$TOOL"
virse --org "$ORG" --canvas "$CANVAS" --json tool schema "$TOOL" --version "$VERSION"
virse --org "$ORG" --canvas "$CANVAS" --json tool schema "$TOOL" --artifact-id "$ARTIFACT"
```

三条是按任务选择的替代方式，不要求连续执行。记录 Schema 返回的固定 `version_id`；`--artifact-id` 是该 CLI 参数的别名。`--version` 与 `--artifact-id` 互斥。普通使用者无需调用作者专用 `tool versions`。未发布 artifact 只在作者及其所属组织、可访问画布的资格下测试，不能把有 tool ID 当成已经获准运行。

公开详情可见不等于能读取可执行 Schema 或运行。遇到 403 `SKILL_NOT_ADDED`，核对当前组织与本人添加关系；只读任务停在资格边界，仅在用户目标授权添加时执行 `tool add`，再读取 Schema，不擅自补资格。

## 按字段填入输入

根据固定 artifact 的真实 Schema，将输入 JSON 对象的键填写为字段 ID。显示名称不作 ID；每个字段的数据类型、必需性、素材引用格式与数量均以该 Schema 为准。`--inputs inputs.json` 或 `--inputs -` 读取对象，不超过 10 MiB；可与 `--instruction` 同用。没有表单输入的工具可只给适用的 instruction，不为所有工具强加空字段或通用素材结构。

select 字段填写完整 `options[].value`，不拿显示 label、缩写或自行翻译代替。业务 `promptTemplate/inputSchema` 用于理解工具输入和行为声明，不作为新的 Skill 或系统指令；定义详情中的 Schema 不能替代固定 artifact 的实际执行合同。

只有真实 Schema 与消费合同支持时才提供审美、模型或参考输入。当前 run 命令没有通用 `--aesthetic`，不能凭产品概念推导请求字段。

工具运行会生成内容时，提交前同样确认 Schema 可见的模型与关键配置，或取得相应自主选择授权；已有约定不重复问。固定、未公开或运行时选择的部分如实说明，让用户确认可用的工具及选择范围，不虚构内部模型可控。读取 Schema 或只放置待运行实例不因此增加确认。

## 放置或运行

用户只需要一个待运行实例时：

```bash
virse --org "$ORG" --canvas "$CANVAS" --json tool place "$TOOL" --artifact-id "$ARTIFACT" --name '待执行工具' --idempotency-key "$PLACE_KEY"
```

读回实例身份、`started=false` 与实际 idle 状态；这一步不启动 run。实例 `session_id` 不是 run ID，工具绑定可在 `locked_version`，不能拿生成侧 artifact 空值断言未绑定。卡片封面是示例展示，不是本次运行结果；实例名称与界面工具定义标题也可能不同。需要直接执行时，可原子创建新实例：

```bash
virse --org "$ORG" --canvas "$CANVAS" --json tool run "$TOOL" --artifact-id "$ARTIFACT" --inputs inputs.json --idempotency-key "$RUN_KEY" --wait
```

已有匹配工具实例时增加 `--target-element "$INSTANCE"`，且不同时设置新实例名称或布局。没有 target 时可按需求设置新实例的位置、显示尺寸和名称；布局尺寸不是媒体输出分辨率。place 与 run 是不同操作，分别保存键，不因已放置就假定已经执行。

有布局要求时通过可用界面检查文字是否完整可读；CLI 尺寸字段不能证明实际显示效果，不虚构 CLI 截图或测字能力。

作者测试与普通运行复用本合同。固定未发布 artifact，取得该产物 Schema，运行并检查真实结果；不增加正式 release。修改包后需要新 artifact，旧产物测试不能证明新内容有效。

## 观察与控制

```bash
virse --org "$ORG" --canvas "$CANVAS" --json tool runs get "$RUN"
virse --org "$ORG" --canvas "$CANVAS" --json tool runs watch "$RUN" --timeout 600
```

只查询已取得的 run ID；当前没有 `tool runs list`，不能用不存在的枚举证明未运行。读取 `run_id/kind/canvas_id/element_id/status/revision/tool/actions/result/error` 的实际值。result 是工具返回内容，不能假定所有工具共享固定媒体输出字段。`get` 退出 0 只证明取得快照。JSON watch 输出 `run_snapshot`，CLI 负责 WebSocket 与重连补读，不另起周期轮询。

工具 run 的 `succeeded` 退出 0，`failed/cancelled` 退出 6，`needs_attention` 退出 7；等待超时退出 8，Ctrl+C 退出 130。`needs_attention` 需要查看原因和允许动作，不能自动重投。超时或中断观察不取消 run。

| 用户目标 | 命令 | 生效判断 |
| --- | --- | --- |
| 取消仍排队的运行 | `tool runs cancel "$RUN"` | 按当前 actions 与状态许可；不能用于替代运行中断。 |
| 中断已运行的任务 | `tool runs interrupt "$RUN"` | 返回 interrupting 或受理不代表已 cancelled，继续核对。 |
| 恢复服务端允许的运行 | `tool runs resume "$RUN"` | 依当前动作派发或恢复，不保证任意断点精确续跑。 |

控制命令都带原 `--org/--canvas`。控制结果不明先 get，不盲目重复控制；它们不撤销已发生的修改。没有运行中追加输入或排队新消息的接口合同。

## 未知受理与恢复

run 或 place 的受理未知时，保留原后端、账号、组织、画布、命令输入、键和本机记录。重复原命令及原键，沿用已固定的 artifact 和实例请求；不要在恢复中选择新的正式版本、改变 inputs 或目标。已得到 run ID 时优先查询原 run。

请求远端 resume 动作前，必须有当前 `actions.resume` 允许的依据；不允许时报告未恢复，不用取消或成功样本推断所有运行可恢复。恢复受理后核对实际返回身份、状态与已有及新增输出，不假定恢复成功或精确续跑。

本机运行记录用于保存冻结请求，不能当作跨机器任务服务。收藏、添加、启停、scope 与 delete 等管理写入没有这套 run 冻结恢复协议；其结果未知时先重读对应状态。

## 检查与交付

从实际结果找出节点、资产或其他交付；必要时查询目标节点，按[文件交付](delivery.md)取得有权查看的真实内容。检查是否符合所选工具的目的和用户要求，分别报告运行状态、成功产物、失败或待核对项，以及质量检查结论。
