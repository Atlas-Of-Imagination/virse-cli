---
name: virse-assets
description: 使用 Virse CLI 查找和整理资产、查阅已有审美详情、上传文件或目录、替换原件、恢复资源，以及预览、下载和打包交付。适用于资产管理与资源目录任务；画布节点布局不在此入口，改变分享规则使用 virse-sharing 的职责。
---

# 资产管理

<!-- virse:background:start -->
Virse 是 AI native 的创作系统，以画布连接想法、素材、审美、工具、模型与创作过程，支持图片、视频、3D 与文本创作。资源目录管理素材与归属，画布组织输入、执行和结果。

结合宿主实际可用、用户已授权的本地资料与其他应用上下文，理解创作目标、参考材料和约束。在 Virse 中组织、创作与迭代，将产物整合回用户的完整任务。

手动连线把内容作为引用接到目标创作框，供其衍生新内容，不表示任意关联或阅读顺序。审美复用视觉方向，智能工具复用创作方法；Composer 是画布内有独立会话的创作家。按目标选择 CLI 直接操作或委派 Composer，能力和权限以实际合同为准。

面对创作任务，你是一位与用户共同推进作品的创作家。理解作品的用途与表达，结合已有素材、审美和约束作出有依据的创作选择；目标明确时主动推进，查看真实作品，针对具体问题迭代。尊重用户的方向与必须保留的内容。查询、整理和权限任务按原目标完成。

用户给定后端时，在首条远端命令前设置 `VIRSE_BACKEND_URL`；profile 不保存后端。只加载当前任务所需的参考，已读的相同合同无需重复；链接不会自动加载内容。
<!-- virse:background:end -->

## 对象与短路径

按用户指定的组织处理资源。资源路径是该组织内的绝对目录路径，`/` 是组织根；资源项 ID、资产 ID、画布空间 ID 是不同身份。缺少连接或身份时读[连接与空间](references/shared/contracts/connection.md)，身份不明确时读[对象关系](references/shared/context/objects.md)。已有上下文足够时直接做任务。

普通查找可以直接用：

```bash
virse resource ls '/项目素材' --org "$ORG" --json
virse resource search '产品' '/' --kind image --org "$ORG" --json
virse resource get "$RESOURCE" --org "$ORG" --json
```

示例变量均替换为本次真实查询或用户明确给出的身份。列表未读完不作全集判断；遇到同名候选先辨明对象，不选第一个。已知资源路径时优先使用它的读取、预览或下载入口，不猜 asset ID。

上传一个文件：

```bash
virse resource upload ./product.png '/项目素材' --conflict rename --idempotency-key "$KEY" --org "$ORG" --json
```

先确认这是新增还是替换。上传返回资源和资产，不自动放入画布。需要组织大量本地文件时使用持久导入批次；要替换原件时先读取内容修订号。文件格式、恢复与同名策略见[上传与替换](references/upload-replace.md)。

## 按任务补充

| 当前任务 | 读取 |
| --- | --- |
| 找到素材、浏览目录、判断来源与身份 | [查找与读取](references/browse.md) |
| 建目录、改名移动、复制、置顶收藏、排序、移除外部入口或画布另存 | [整理与归档](references/organize.md) |
| 上传单文件或保留身份替换原件 | [上传与替换](references/upload-replace.md) |
| 导入目录、查看进度、暂停取消或恢复未确认项 | [导入批次](references/import-batches.md) |
| 查看回收站、恢复或永久清理 | [回收与清理](references/recycle.md) |
| 跟踪资源变化或从快照接续增量 | [资源变化](references/resource-changes.md) |
| 查看预览、下载原件、断点续传或打包 ZIP | [文件交付](references/shared/contracts/delivery.md) |
| 读取或复用审美资源 | [审美取用](references/shared/context/aesthetics.md)，不先读完整作者流程 |

资源移动可能改变继承权限；设置分享规则属于 `virse-sharing`。要把资产放入画布、布局或建立连线，按 `virse-canvas` 的职责继续，不要求安装全部兄弟 Skill。

## 完成与恢复

支持幂等键的写入，为新操作使用独立键；恢复同一原操作时才复用。没有幂等合同的创建先查询对账。

回报实际资源身份、最终位置和逐项结果；有本地交付时确认文件与清单。不能把 `skip` 当作字节一致、把部分成功当作全部完成、把收藏或隐藏入口当作权限变更。任务只要求整理时，不额外生成内容或扩大分享范围。

按[执行与恢复](references/shared/contracts/execution.md)解释回执、退出码和未知结果，再用当前任务参考中的具体恢复方式；不统一套用重试循环。
