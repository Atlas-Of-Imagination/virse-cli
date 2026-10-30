---
name: smart-tool-creator
description: 使用 Virse CLI 在本地创作、修改、校验和发布 Smart Tool，并在画布上验证行为。适用于创建工具包及迭代已有工具，不用于管理 Virse 内置 Creator 会话。
---

# 本地创作 Smart Tool

本地目录是创作工作区。由你理解需求、编写文件和判断输出质量，Virse 后端负责包校验、权限和版本一致性。不要调用 `/skill-creator/session` 或伪造 Creator 交付记录。

先读取当前 `virse tool --help` 和相关子命令帮助，确认客户端实际支持的命令。后端地址通过 `VIRSE_BACKEND_URL` 指定，默认 production；认证使用现有 API token 配置，不把 token 写入包或示例。使用用户指定的组织与画布。

## 创作与修改

- 新工具使用 `virse tool init <directory>`。已有工具用 `tool pull <id> --output <new-directory>` 拉取，指定历史版本时保留它与当前发布基线的区别。
- `SKILL.md` 提供非空的 `name`、`description` frontmatter 和执行说明。`when_to_use` 是发现提示，建议填写，但字数和固定章节不是发布条件。
- 可选 `interface.json` 描述输入。包内的脚本、参考文件和资源应与说明一致；不要把凭据、依赖缓存或本地工作日志打包。
- 根据用户实际任务选择说明结构，不为通过检查而机械补齐固定的五段标题。模型编写脚本后，是否执行本地脚本应遵循当前任务授权。

## 验证与交付

先运行 `virse tool validate <directory>`，需要权威后端报告时运行 `virse tool validate <directory> --remote --org <id> --json`。修复 errors；warnings 逐项判断，不必为了清除警告改变正确设计。静态检查不能证明运行效果正确。

使用 `virse tool build <directory> --org <id> --json` 保存未发布产物，记录返回的工具 ID 与 `artifact.id`。
运行 `virse tool run <tool-id> --artifact-id <artifact-id> --canvas <canvas-id> --org <id> --wait --json`，检查实际结果。作者只可在工具所属组织的可访问画布试运行未发布产物；测试不改变正式版本。
修改后重新 build，后续测试固定新的产物 ID，不能把旧产物的测试结果当成新产物的验证。

发布必须符合用户的交付要求。`virse tool publish --tool-id <tool-id> --artifact-id <artifact-id> --org <id>` 发布已验证的产物；`virse tool publish <directory> --org <id>` 是构建加发布快捷入口。网络结果不确定时按命令提示使用 `--resume`，保持原始包与请求，不盲目重建工具。遇到版本冲突，拉取当前状态并审阅差异，不自动覆盖。

报告工具 ID、正式版本、校验警告、实际运行结果和未验证项。不要声称没有运行的测试已经通过。
