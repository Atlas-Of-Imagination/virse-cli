# 编辑对象与关系

## 创建画布

```bash
virse canvas create '产品方案' --parent '/项目' --org "$ORG" --json
```

父目录必须存在且可写，名称 1–255 字。保留返回的 `canvas_space_id` 与资源目录 `dentry.id`，不自动修改默认画布。创建没有幂等回执，结果不明先列目标目录确认，不自动重复创建。

## 节点创建与字段修改

已有资源优先用 `canvas add`。需要直接构造节点时，按该节点类型已经核实的后端合同准备 JSON；例如文本节点的创建文件可包含 `{"type":"text","text":"方案说明"}`。不要把这个例子推广为所有节点的通用字段表。

```bash
virse canvas element create --file node.json --idempotency-key "$KEY" --org "$ORG" --canvas "$CANVAS" --json
virse canvas element update "$ELEMENT" --file patch.json --idempotency-key "$KEY" --org "$ORG" --canvas "$CANVAS" --json
virse canvas element name "$ELEMENT" '方案说明' --org "$ORG" --canvas "$CANVAS" --json
virse canvas element delete "$ELEMENT" --org "$ORG" --canvas "$CANVAS" --json
```

一次更新补丁只放本次要改的实体字段，例如 `{"layout":{"position":{"x":100,"y":200}}}`；实体 ID 由命令位置参数指定，不在补丁里重复传。名称 1–255 字。删除节点不等于永久清理资产，也不自动取消已运行任务。

## 组合与成员关系

普通组合把一次创作相关的材料、节点和结果放在一起，成员仍独立。按实际节点 ID 准备组合 JSON，例如包含 `elements` 数组和 `title`；创建组合不会创建成员节点。审美组和搜索组有专用行为，不把普通组合的编辑方式直接套用。

```bash
virse canvas group create --file group.json --org "$ORG" --canvas "$CANVAS" --json
virse canvas group update "$GROUP" --file group-patch.json --org "$ORG" --canvas "$CANVAS" --json
virse canvas group name "$GROUP" '候选方案' --org "$ORG" --canvas "$CANVAS" --json
virse canvas group delete "$GROUP" --org "$ORG" --canvas "$CANVAS" --json
```

成员调整和其他可改字段以核实的合同为准，不从原始 JSON 入口推定完整编辑能力。删除组合时，不先声称成员一定保留或连带删除，核对后端结果与实际节点。

## 连线的输入关系

手动输入连线把已有内容作为目标创作框的引用，让目标据此衍生新内容。例如把商品图与文字要求接到空的图像创作框，运行后生成新作品。先确认目标实际支持相应输入；连线用于建立创作依据，不为“看起来有关”而补线。Composer 到产物的系统来源边不属于这种手动输入关系，不推定通用 edge 命令可以维护它。

合法例：把图像参考接到支持图像输入的生成目标；把说明文字接到实际接收该文字的生成输入。文本也能是合法输入，不能按文本节点类型或空 `input_slot` 一概判错。反例：用边表示展览从入口到展区的参观顺序、说明文字解释某张图、素材来自某份文件；这些用文字、分组与空间布局表达。

创建文件可包含 `source_element_id`、`target_element_id`、`input_slot`。普通组合可整体交给 Composer 理解内容与关系；图像、视频或文本创作引用具体成员，不把组合当成合成图片。通过 CLI 使用组合端点前仍确认对应合同；具体 slot 与端点是否可用以目标合同为准，不从可保存推断生成语义正确。

```bash
virse canvas edge create --file edge.json --org "$ORG" --canvas "$CANVAS" --json
virse canvas edge update "$EDGE" --file edge-patch.json --org "$ORG" --canvas "$CANVAS" --json
virse canvas edge delete "$EDGE" --org "$ORG" --canvas "$CANVAS" --json
```

已核实的编辑例为 `{"input_slot":"image"}`。建立或移除连接只是修改图关系，不自动启动或取消生成。事后补线不能证明历史生成使用了该输入，也不能替代任务记录或生成配置的核对。连线没有名称，不能使用 `edge name`。

## 事务与冲突恢复

多对象修改使用 `canvas transaction --file transaction.json`。JSON 含当前 `org_id` 和 1–100 个 `operations`；每项含 `action/id/value/expected_revisions`，动作限于 element、group、edge 的 create/update/delete。已有对象必须带实际服务端修订号；新对象不要求旧修订号。

例如只移动一个节点的完整请求：

```json
{
  "org_id": "00000000-0000-0000-0000-000000000001",
  "operations": [{
    "action": "element.update",
    "id": "00000000-0000-0000-0000-000000000004",
    "value": {"layout": {"position": {"x": 100, "y": 200}}},
    "expected_revisions": {"layout": 2}
  }]
}
```

示例 ID 与修订号须替换为本次实际读取的值，body 的组织须与 `--org` 一致；不要把示例修订号直接用于真实对象。

普通 create/update/delete/name 也通过事务执行。更新与删除可显式传 `--revisions '{"layout":2}'` 等真实修订映射；未提供时 CLI 首次读取并冻结。文件内不能混入 `org_id/expected_revisions`，这些由上下文和选项提供。

```bash
virse canvas transaction --file transaction.json --idempotency-key "$KEY" --org "$ORG" --canvas "$CANVAS" --json
```

服务端返回 mutation_id 和逐操作结果。结果不明使用原请求和原键；不能变更补丁或刷新修订号后强制重放。修订冲突先读当前对象并比较，再决定如何合并。一般恢复边界见[执行与恢复](shared/contracts/execution.md)。
