# 理解与取材

## 找到画布

```bash
virse canvas list --org "$ORG" --json
virse canvas get "$CANVAS" --json
virse canvas contents '/项目/方案画布' --kind image --org "$ORG" --json
```

`list` 通过权限感知目录列画布，最多每页 200 条，按返回 `next_cursor` 保留组织续页。读取目录条目的真实目标身份后再使用画布空间 ID。`get` 读取画布与已保存 elements，展开为 none，不是截图，也不承诺展开资源全文。

`contents` 读取媒体/文档/审美的内容投影，参数是画布资源路径或资源项 ID；支持名称搜索、类型、Word/PDF 子类与排序。按 `has_more` 使用原条件和下一 `offset` 继续，每页最多 200 项。它与节点布局读取分开，也不是快照分页。投影返回 0 项不能证明画布为空，也不能证明没有文本或工具节点；盘点画布时另用 `element list` 读完全部分页，按真实节点类型核对数量与内容。

## 读节点内容与显示布局

```bash
virse canvas element list --offset 0 --limit 100 --org "$ORG" --canvas "$CANVAS" --json
virse canvas element get "$ELEMENT" --org "$ORG" --canvas "$CANVAS" --json
```

用节点 `id/type/name/revisions` 和实际返回的 `attributes` 判断内容，例如 `text/text_rich/style`。查看 `layout.position`、`size`、`rotation` 和 `z_index` 等实际字段，理解画布上的位置、显示尺寸与叠放关系；这些字段不等于图像像素内容或 3D 相机。需要指定区域时，根据已读布局筛出候选再查看内容，不虚构 CLI 区域截图参数。

`element/group/edge list` 都采用 offset 分页，每页最多 200。字段以实际返回为准；必要时缩小范围，不能凭一页推断画布没有其他对象。

## 读组合成员与连线

```bash
virse canvas group list --org "$ORG" --canvas "$CANVAS" --json
virse canvas group get "$GROUP" --org "$ORG" --canvas "$CANVAS" --json
virse canvas edge list --org "$ORG" --canvas "$CANVAS" --json
virse canvas edge get "$EDGE" --org "$ORG" --canvas "$CANVAS" --json
```

组合读取标题、位置、边界与 `elements` 成员 ID；成员 ID 不包含完整节点内容，需要时逐个读取。成员是组合里的节点，不是协作者名单。区分普通组合、审美组和搜索组的用途，按实际返回内容理解，不仅凭位置或名称推断类型。

连线可返回 `source_element_id/source_group_id`、`target_element_id/target_group_id`、`input_slot` 等字段。手动输入边表示把哪些内容交给目标、分别作何用途；Composer 到产物的系统边表示生成来源，两者不混用。一般叙事和阅读顺序仍由文字与布局判断。保存或事后补建的关系不证明下游任务已经执行或历史生成实际使用了该参考，须另核对任务与可读配置。区域名称或用途没有明确标注时，说明它是基于当前文字、布局或内容的推断。

## 读内容绑定与生成配置

`content_ref.kind/id` 表示节点当前绑定的资产或审美身份。要看真实图片、视频或文档，按[文件交付](shared/contracts/delivery.md)取得授权展示或原件；取用审美时读[审美取用](shared/context/aesthetics.md)。不能把当前绑定对象说成最初上传原件。

`generation.config` 以及实际返回的 `artifact_id/artifact_version_id` 可用于理解当前配置。不要先假定 prompt、参数、参考字段一定齐全；外部上传图可能完全没有生成配置。指定任务的历史原输入也没有完整保证。需要继续生成时交付确实读到的配置、真实参考身份与用户保留项，不由图像内容猜造原 prompt。
