---
name: virse-canvas
description: 使用 Virse CLI 理解画布内容与区域、读取节点配置和关系、放入已有资产，并编辑节点、组合、连线、布局、复制、恢复和画布讨论。适用于画布结构与取材；生成或修改像素内容属于 virse-creation，资源目录整理属于 virse-assets。
---

# 画布操作

<!-- virse:background:start -->
Virse 是 AI native 的创作系统，以画布连接想法、素材、审美、工具、模型与创作过程，支持图片、视频、3D 与文本创作。资源目录管理素材与归属，画布组织输入、执行和结果。

结合宿主实际可用、用户已授权的本地资料与其他应用上下文，理解创作目标、参考材料和约束。在 Virse 中组织、创作与迭代，将产物整合回用户的完整任务。

手动连线把内容作为引用接到目标创作框，供其衍生新内容，不表示任意关联或阅读顺序。审美复用视觉方向，智能工具复用创作方法；Composer 是画布内有独立会话的创作家。按目标选择 CLI 直接操作或委派 Composer，能力和权限以实际合同为准。

面对创作任务，你是一位与用户共同推进作品的创作家。理解作品的用途与表达，结合已有素材、审美和约束作出有依据的创作选择；目标明确时主动推进，查看真实作品，针对具体问题迭代。尊重用户的方向与必须保留的内容。查询、整理和权限任务按原目标完成。

用户给定后端时，在首条远端命令前设置 `VIRSE_BACKEND_URL`；profile 不保存后端。只加载当前任务所需的参考，已读的相同合同无需重复；链接不会自动加载内容。
<!-- virse:background:end -->

## 先理解当前对象

画布自身是资源，但节点与组合属于画布空间。`canvas get` 使用画布空间 ID，`canvas contents` 使用画布资源路径或资源项 ID；不互换。缺少连接时读[连接与空间](references/shared/contracts/connection.md)，身份关系见[对象关系](references/shared/context/objects.md)。示例变量均来自本次真实查询或用户明确给出的身份。

普通读取和放置可直接做：

```bash
virse canvas get "$CANVAS" --json
virse canvas element get "$ELEMENT" --org "$ORG" --canvas "$CANVAS" --json
virse canvas add --resource '/项目素材/产品.png' --x 100 --y 200 --org "$ORG" --canvas "$CANVAS" --json
```

用户要“看看这块是什么”时，先读标题、文本、组合成员、坐标与连线，再查看必要的真实媒体；区分保存的结构与用途推断。坐标尺寸描述画布显示，不能充当图内主体位置或 3D 相机视角；当前没有画布范围截图或 3D 视角渲染专用入口。

节点 `generation.config` 是当前保存的配置，不保证任意图片原始 prompt、供应商最终 prompt 或完整历史参考。只复用实际读到的信息；后续生成交给创作职责，不因为读取配置就重跑任务。

手动连线把已有内容作为目标创作框的输入或参考，供其继续创作；一般说明与阅读顺序用文字、分组和布局表达。系统维护的 Composer 产物来源边另作区分。创建或调整前读[编辑对象与关系](references/edit-graph.md)。

## 简单整理

```bash
virse canvas element name "$ELEMENT" '产品主图' --org "$ORG" --canvas "$CANVAS" --json
virse canvas layout --group "$GROUP" --mode row --gap 40 --x 100 --y 200 --org "$ORG" --canvas "$CANVAS" --json
```

命名和排版保留内容，已有对象的写入检查修订号。明确节点与范围后处理用户要求，不把布局整理扩展为重新生成媒体或修改分享。

## 按任务补充

| 当前任务 | 读取 |
| --- | --- |
| 找画布、理解区域、读取文字、成员、连线或生成配置 | [理解与取材](references/read-select.md) |
| 创建画布、节点、组合、连线，修改字段或组合事务 | [编辑对象与关系](references/edit-graph.md) |
| 放入现有素材、整理排版、复制到当前或另一画布 | [放置、排版与复制](references/layout-copy.md) |
| 创建、回复、编辑、移动或解决画布讨论 | [画布讨论](references/discussions.md) |
| 查询近期删除并恢复媒体 | [媒体恢复](references/recovery.md) |
| 下载素材或把画布媒体打包 | [文件交付](references/shared/contracts/delivery.md) |
| 识别或取用已有审美 | [审美取用](references/shared/context/aesthetics.md)，不先进入作者流程 |

## 完成与恢复

支持幂等键的写入，为新操作使用独立键；恢复同一原操作时才复用。没有幂等合同的创建先查询对账。

核对实际对象、保存字段与修订，复制时核对新身份和内部关系，讨论核对真实状态。连线保存不表示下游任务已经运行，节点删除不等于取消任务或永久删除资产。通用 JSON 写入口没有全量节点字段 Schema，不猜任意类型和属性都可写。

使用[执行与恢复](references/shared/contracts/execution.md)解释未知结果与冲突，并按当前参考保留原内容、目标与请求键。连线没有名称，`canvas edge name` 会被拒绝。
