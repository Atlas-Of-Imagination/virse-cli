---
name: virse-aesthetic-creator
description: 从 Virse 画布发现审美方向，或用参考素材创建、训练和调优审美，修订获准草稿、固定快照测试并保存。适用于审美作者；已有审美查阅走 virse-assets。
---

# 审美制作

依赖 `virse-base`。首次使用本技能时，从宿主技能列表定位并读取 base 的 `SKILL.md`；已读且适用的内容沿用。下文 `virse-base/references/...` 均相对于该技能的实际目录，按任务读取，不假定安装在全局目录。缺少 base 时先补齐同来源、同版本、同安装范围的 base，再执行依赖它的 Virse 操作。

用户指定后端时，在首条远端命令前设置 `VIRSE_BACKEND_URL`。

审美将参考图的视觉方向保存为可复用条件。你承担审美作者的职责：判断用户意图、参考材料与效果，直接修订获准的草稿并组织测试。后端负责素材读权、源资源编辑权、测试画布归属、修订冲突、训练与保存。使用公共作者门面，不启动内置调优 Agent 替代自己的判断，不伪造 `running` 或交付状态，不创建本地审美包或另一套正式版本。

从画布理解、总结或提升审美方向时，先读[从画布发现并制作审美](references/from-canvas.md)，主动看作品、推荐候选并说明依据；指定区域是可选输入。已明确的训练或保存操作直接选对应参考。

| 当前任务 | 直接读取 |
| --- | --- |
| 看复杂画布，理解视觉偏好、推荐发展方向并提炼成审美模块 | [从画布发现并制作审美](references/from-canvas.md) |
| 查详情或准备使用已有审美 | 审美身份与普通取用（`virse-base/references/context/aesthetics.md`）；不读取作者私有正文 |
| 创建、更新展示信息、替换参考图、发起或观察训练 | [创建与训练](references/create-and-train.md) |
| 创建测试现场、读取草稿、修订正文、固定快照并比较效果 | [草稿与测试](references/draft-and-test.md) |
| 查看或下载测试作品 | 文件交付（`virse-base/references/contracts/delivery.md`） |
| 保存选定修订、存入资源目录、恢复旧修订或清理测试画布 | [保存与清理](references/save-and-cleanup.md) |

需要认证或切换后端时读连接与范围（`virse-base/references/contracts/connection.md`）；测试生成读生成合同（`virse-base/references/contracts/generation.md`），以当前 operation 和模型实际支持的字段绑定审美。测试生成沿用已确认的配置与执行范围或适用的自主授权；缺少时一次确认模型、关键配置及含修正版的数量范围，范围内不重复问，超出范围再确认。

需要分析图片参考、设计测试提示词或诊断视觉效果时，按需读取 `virse-image-craft`；视频测试的模式、运动与连续性使用 `virse-video-craft`。仅训练、查询修订或保存草稿时不预读技巧；实际生成与审美绑定仍遵守本入口合同。

`aesthetic call` 的 `--data` 是该动作的 HTTP JSON 对象，不自动注入全局 `--org/--canvas`；`draft/context` 的 GET 不发送 body。使用参考中的真实字段和已查询身份，不凭动作名猜 JSON。命令或路由不存在、请求合同不匹配时核对客户端与后端能力，不能改走私有旧接口绕过。

训练 run、源 style、草稿 style、会话 revision 和测试快照分开记录。完整正文修订使用最新读取的 `expected_revision`；冲突先重读比较，不只替换数字强写。训练完成、效果合格、草稿保存、资源落盘分别核对，最终说明实际完成和未验证部分。
