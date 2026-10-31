# 产物测试与发布

## 构建未发布产物

包已写好并完成必要校验后，用用户指定组织构建：

```bash
virse tool build ./product-shot-tool --org <org-id> --json
```

构建创建或复用工具身份，上传完整包并返回不可变 artifact，记录返回的 `skill.id` 与 `artifact.id`。已有源码绑定与发布基准应保留。build 不执行脚本、不产生真实运行结果，也不发布正式 release。

目录绑定和恢复回执由 CLI 保存；不要删除本地恢复记录、换目录或换组织来绕过中断。修改后的包需要新的 artifact，后续测试与发布使用该新身份。

## 固定产物测试

读取 build 返回的确定 artifact 的输入 Schema，按字段 ID 准备输入，再运行同一产物：

```bash
virse tool schema <tool-id> --artifact-id <artifact-id> --canvas <canvas-id> --org <org-id> --json
virse tool run <tool-id> --artifact-id <artifact-id> --inputs ./test-inputs.json --idempotency-key "$TEST_KEY" --canvas <canvas-id> --org <org-id> --wait --json
```

运行前保存本次 `TEST_KEY` 和输入；同次未知受理保持同一输入与同一键，新测试使用新键。无表单工具可按实际合同提供 `--instruction`。输入来源、目标节点、幂等与 run 观察见[运行合同](shared/contracts/tool-run.md)，不另做一套轮询或重连。

未发布 artifact 仅由作者在工具所属组织的可访问画布试运行；不能把团队可见或能用某正式版本理解成能测试作者草稿。记录 tool、artifact、run、输入与输出身份，查看真实图片、文本或其他产物，检查用户关心的行为和质量。运行 succeeded 与作品符合要求分别判断。

如果运行需要继续观察，沿用原 run：

```bash
virse tool runs get <run-id> --canvas <canvas-id> --org <org-id> --json
virse tool runs watch <run-id> --canvas <canvas-id> --org <org-id> --json
```

测试发现问题时修改本地包、重新 build、重新测试。不要将旧 artifact 的测试结论附给新 artifact；不把测试成功说成后端发布接口已强制验过质量。

## 发布指定产物

用户要求正式交付时，发布已测试的同一 artifact：

```bash
virse tool publish --tool-id <tool-id> --artifact-id <artifact-id> --org <org-id> --json
```

按明确要求选择 `--scope personal|team`，不因测试顺手扩大可用范围。命令要求作者身份，使用 artifact 自带的发布基准；检查回执的 release、正式版本与 artifact 对应。显式 artifact 模式不接受 `--resume`。

目录式入口是构建并发布快捷方式：

```bash
virse tool publish ./product-shot-tool --org <org-id> --json
```

它可能构建当前目录的新内容；要确保发布的是刚才测试的 artifact，优先使用明确 tool/artifact 的入口。包修改后不能只因目录相同就宣称发布了原测试内容。

## 中断与冲突恢复

| 情况 | 怎么处理 |
| --- | --- |
| 目录 build/publish 响应不确定 | 使用同一目录、身份、组织和原恢复记录接续；按 CLI 提示重跑原命令。包已改变但仍要完成旧冻结请求时，目录模式使用 `--resume`。 |
| 显式 artifact publish 响应不确定 | 保持原 tool、artifact、组织和 scope，重放相同发布请求并核对回执；不能添加 `--resume`。 |
| 原范围与待处理回执不同 | 先完成或核清原请求，不通过更换 scope 重放。 |
| 源码或发布基准冲突 | 读取当前状态与差异；必要时按[源码与作者管理](source-management.md)显式放弃已知草稿，拉到新目录并重新应用修改，不自动覆盖。 |
| 测试观察断线或超时 | 找回原 run 并读取状态，不因观察失败重新运行收费任务。 |

CLI 的目录回执处理幂等和冻结请求，不保证已经完成远端发布；以返回的正式 release 为证据。详细未知状态处理见[执行与恢复](shared/contracts/execution.md)。实际文件交付见[交付合同](shared/contracts/delivery.md)。
