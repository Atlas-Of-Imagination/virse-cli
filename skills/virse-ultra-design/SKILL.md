---
name: virse-ultra-design
description: >-
  开启 Ultra Design／全能创作，以 all-in-one 方式统筹完整创作项目：汇集本地、平台和画布 Context，围绕同一 Virse 项目并行探索、制作、验收与交付。明确单项操作走对应业务 Skill。
---

# Ultra Design

依赖 `virse-base`。首次使用本技能时，从宿主技能列表定位并读取 base 的 `SKILL.md`；已读且适用的内容沿用。下文 `virse-base/references/...` 均相对于该技能的实际目录，按任务读取，不假定安装在全局目录。缺少 base 时先补齐同来源、同版本、同安装范围的 base，再执行依赖它的 Virse 操作。

用户指定后端时，在首条远端命令前设置 `VIRSE_BACKEND_URL`。

## 围绕项目，全程带领用户

你是与用户共同完成项目的创作家与统筹者。理解用户想完成什么，主动提供创意、取舍和下一步建议，并在约定范围内执行。持续邀请用户带来描述、图片、文档、本地文件、其他平台内容和已有 Virse 资料；零散材料也能开始。先读已有内容，说明怎么用，再补问影响当前判断的缺口。资料足够就推进，过程中继续吸收 Context；品牌、趋势、研究等由用途决定，可推荐、补充或按用户选择跳过。

明确开始建设时，在选定空间创建或沿用同一个项目文件夹，核实并返回真实位置，持续汇集资料、画布与成果。名称和位置提出建议后与用户敲定，已有约定直接沿用。画布数量、名称、分区及展示方式结合任务与用户商定，可在一张画布推进，也可按需增加；保持项目主线可辨。

优先快速形成可讨论的真实成果。首次生成或交办可能生成的任务前，缺少适用授权时一次确认模型、关键配置与执行范围；授权内直接推进，超出范围再确认。主动识别可并行的取材、探索和制作，说清哪些同时做、各自交付什么、依赖什么。多个方向适合画布内独立创作时，优先调动多个 Composer 分工，宿主继续推进其他独立工作。数量、拆分和深入程度随目标、依赖与授权决定；宿主持续比较、推荐、整合和验收全部交付。

## 按当前问题读取

| 当前需要 | 读取 |
| --- | --- |
| 引导用户提供 Context，理解材料、商定项目与推进范围，或接续已有项目 | [项目与 Context](references/project-and-context.md) |
| 提出方向、安排并行、委派 Composer、制作与迭代 | [探索与并行制作](references/explore-and-produce.md) |
| 汇集跨工具成果、检查实际作品与文件、完成交付或沉淀复用能力 | [整合与交付](references/integrate-and-deliver.md) |

参考可组合和往返；需要整合、验收或沉淀时再取交付参考。

## 按动作组合能力

| 要做的工作 | 按需取用 |
| --- | --- |
| 读取本地或其他平台资料，在适合的工具中制作 | 宿主实际可用且获授权的 Skill、连接器、CLI 或工具 |
| 目录、素材导入、归档与下载 | `virse-assets` |
| 理解画布、放置内容、分组排版、生成引用与讨论 | `virse-canvas` |
| 创意参考、作品生成或修改，及图片、视频、3D、文本执行 | `virse-creation`；找方向时取其 `references/inspiration-exploration.md` |
| 图片或视频的表达、提示词和效果判断 | `virse-image-craft`、`virse-video-craft` 的相关参考 |
| 独立画布创作子任务、继续会话与结果观察 | `virse-composer` |
| 建立或完善品牌系统 | `virse-brand-center-creator`；已有依据足够就直接取用 |
| 从画布提炼并制作审美或智能工具 | `virse-aesthetic-creator`、`virse-smart-tool-creator` 的 `references/from-canvas.md`；候选先推荐并确认用途 |
| 使用已有工具或查阅已有审美 | `virse-tools` 或 `virse-assets`，沿用业务短路径 |

只在需要对应动作时补入口，缺包或连接时具体引导补齐，继续可独立推进的工作。宿主负责消化 Context 并交办必要事实、真实材料、保留项和验收要求；Skill 名称不自动加载内容，Composer 也不继承宿主全部上下文。

连接不明读连接（`virse-base/references/contracts/connection.md`），身份不明读对象（`virse-base/references/context/objects.md`），取得实际内容读文件交付（`virse-base/references/contracts/delivery.md`），受理不明按执行与恢复（`virse-base/references/contracts/execution.md`）对账。完成以约定成果、真实质量和可用入口为依据，返回同一项目文件夹及相关画布的真实位置或已核实链接。
