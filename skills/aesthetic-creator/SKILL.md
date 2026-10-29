---
name: aesthetic-creator
description: 使用 Virse CLI 的现有审美训练、草稿调优、测试和保存能力完成审美创作。适用于从已有素材创建审美及迭代已有审美，不用于导入本地审美包或构建新版本体系。
---

# 审美创作

先读 `virse aesthetic --help`。通过现有 API token 登录，后端地址由 `VIRSE_BACKEND_URL` 指定。不要把 token 写进请求文件、素材或回答。

你负责理解用户意图、判断素材是否充分、检查结果和组织迭代。后端负责资源权限、格式、修订冲突与保存规则。不要启动内置 Agent 对话，也不要伪造它的运行状态。

- 创建／更新、查看和保存资源使用 `virse aesthetic call create|details|save-resource --data '<请求 JSON>'`。JSON 是现有 HTTP 请求参数，不是审美文件格式。
- 有画布上下文时使用 `virse aesthetic train --canvas <id> --org <id> --data '<现有 CreateStyleRequest>' --idempotency-key <唯一键>`。同键只能重放完全相同的请求；结果未知时不要换键盲目重试。
- `virse aesthetic get|watch|cancel|resume <run-id> --canvas <id> --org <id>` 查询或观察训练。watch 使用 WebSocket；Ctrl+C 只停止观察。取消仅适用于未投递的任务。
- 通过 `call test-canvas` 为已有审美创建现有私有测试画布，再用 `call start-draft` 建立草稿上下文。它不是模型对话。
- `call context --id <session-id>` 读取被授权的正文和参考素材；自行检查需要修改哪些部分。
- `call revise --id <session-id> --data '{"expected_revision":0,"summary":"完整修改后的正文"}'` 只修改草稿。每次使用最新读取的 revision；冲突时重新读取、比较并决定如何合并，不自动覆盖。
- 使用 `call snapshot` 固定某次现有草稿内容，配合现有画布与图像生成命令验证效果；保留哪些图像来自哪次测试的记录。不把静态检查当作效果验证。
- 用户要求保存时使用 `call save-draft`，携带 expected_revision 和可选 version。新资源保存到资源目录仍使用 `call save-resource`；按现有权限和用户指定位置保存。
- `call restore` 恢复会话内已有修改。不要把会话修订号称为正式发布版本。

具体字段见仓库 README 的审美章节和命令帮助。不要自行上传本地包、增加素材或扩大共享范围。报告实际完成的训练、测试、保存和未验证项。
