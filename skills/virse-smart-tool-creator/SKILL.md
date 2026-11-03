---
name: virse-smart-tool-creator
description: 从 Virse 画布自动发现可复用任务、总结工作流并制作智能工具；使用 CLI 创建和修改原生包，校验、构建确定产物，在画布上测试并按要求发布。适用于工具作者；运行或管理已有工具不需要进入包制作流程。
---

# 智能工具制作

依赖 `virse-base`。首次使用本技能时，从宿主技能列表定位并读取 base 的 `SKILL.md`；已读且适用的内容沿用。下文 `virse-base/references/...` 均相对于该技能的实际目录，按任务读取，不假定安装在全局目录。缺少 base 时先补齐同来源、同版本、同安装范围的 base，再执行依赖它的 Virse 操作。

用户指定后端时，在首条远端命令前设置 `VIRSE_BACKEND_URL`。

你在这里承担工具作者的职责：把可复用需求变成完整原生包，定义输入和行为，测试固定产物并判断输出。平台负责包校验、权限、执行和版本一致性；不启动内置 Creator 重做已编写的包，不调用旧创建会话或伪造交付记录。

从画布发现或总结工具时，先读[从画布发现可复用任务](references/from-canvas.md)，主动浏览、提出候选并说明依据；指定区域是可选输入。只处理已写好的工具包时直接选对应操作参考。

配套作者 Skill 的 `SKILL.md` 与将被交付工具包中的 `SKILL.md` 是不同对象。新工具从 `virse tool init <directory>` 开始；修改已有工具先拉取本人源码，不从运行实例反推完整包。

| 当前任务 | 直接读取 |
| --- | --- |
| 看复杂画布，发现值得复用的流程、推荐并总结成智能工具 | [从画布发现可复用任务](references/from-canvas.md) |
| 初始化、写说明、输入表单、脚本和素材，或处理校验报告 | [原生包与校验](references/native-package.md) |
| 构建、测试确定产物、发布和恢复不确定结果 | [产物测试与发布](references/artifact-lifecycle.md) |
| 拉取历史源码、查看正式版本、放弃已知草稿、准备封面 | [源码与作者管理](references/source-management.md) |

本地 `init` 与本地 `validate` 无需登录；需要远端操作或身份修复时读连接与范围（`virse-base/references/contracts/connection.md`）。复用确定工具产物时读运行合同（`virse-base/references/contracts/tool-run.md`）；需要交付实际文件时读交付合同（`virse-base/references/contracts/delivery.md`）。不预读所有参考。

先明确工具用途、必需输入和可观察输出，再编写。build 不执行包内脚本，也不发布正式 release；运行验证必须固定 build 返回的 artifact，查看真实结果。测试涉及生成时，沿用适用授权；缺少时先确认可见配置与执行范围，不把固定工具内部模型当作可选择参数。包有变化后重新 build，不把旧产物测试当成新包测试。

工具涉及图片或视频的创作方法、提示表达与效果测试时，按需使用 `virse-image-craft` 或 `virse-video-craft`；将必要方法落实到工具包，宿主仍负责实际验收。仅校验、构建或发布已有包时不加载创作技巧。

用户要求发布时才发布相应范围。目录模式的 `--resume` 与显式 artifact 发布是两套恢复入口，不能互用。交付时说明 tool、artifact、实际测试输入和结果；发布过再给 release 与正式版本，保留未验证项。
