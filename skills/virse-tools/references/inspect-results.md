# 检查工具结果

## 关联运行与实际产物

```bash
virse --org "$ORG" --canvas "$CANVAS" --json tool runs get "$RUN"
```

分别读取本次 run、实例节点和真实输出，确认 tool、固定 artifact 与各自身份；实例 session ID、绑定工具 artifact 与输出 generation artifact 不能互换。卡片封面不算输出。result 是工具实际返回的数据，不能假定它必定是图片 URL 或统一 outputs 数组；从明确结果取得真实节点、资产或其他交付。存在关联生成任务时分别核对其状态，不把外层 Agent/run 已结束当作所有内层生成都成功。

必要时按确切节点读取：

```bash
virse --org "$ORG" --canvas "$CANVAS" --json canvas element get "$ELEMENT"
```

工具内部 Agent 的完成陈述需要用真实节点名称、正文、资产和输出状态核对；其执行中加载的业务 Skill 不等于外部调用者又加载了本地 Skill。工具内部 Agent 执行与通过 Composer 委派是不同路径，不因看到内部对话就改用 Composer。

随后依[文件交付](shared/contracts/delivery.md)取得有权限的预览或原件。不要根据工具名称、输入表单或运行日志推断实际文件已经存在。

## 按任务要求验收

布局有明确要求时检查实际显示与文字可读性，不虚构 CLI 截图或测字接口。对照用户目标与本次 Schema 输入，检查真实产物的内容、保留项及所需格式。视觉产物必须实际查看，静态预览不证明整段视频或完整三维结构；无法查看时明确尚未验证的质量。

run、实例与媒体输出的状态分别判断：媒体已完成时 run 仍可能 running，run cancelled 也不表示已有输出被撤销。运行失败或 needs_attention 时保留可用产物，按[运行合同](shared/contracts/tool-run.md)判断可用动作。若运行成功但效果不合适，依据具体差异调整输入并建立新的明确运行，不把效果调整伪装为未知受理恢复。用户要求修改工具本身的可复用逻辑时才切作者职责。

交付中区分运行状态、实际产物、质量判断和保存/下载结果。作者测试还需记录确定 artifact 和测试输入；测试结论不能移到未经运行的新包或新 artifact 上。
