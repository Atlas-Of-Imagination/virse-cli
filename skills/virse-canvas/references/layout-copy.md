# 放置、排版与复制

## 放入已有资产

```bash
virse canvas add --resource '/项目素材/产品.png' --name '产品参考' --x 100 --y 200 --width 512 --height 512 --idempotency-key "$KEY" --org "$ORG" --canvas "$CANVAS" --json
virse canvas add --asset "$ASSET" --org "$ORG" --canvas "$CANVAS" --json
```

选择资源路径/资源项 ID 或 asset ID，两者互斥。放入后在目标画布形成使用实例，来源资源保留，实例的位置与连线单独管理；取用内容不等于复制完整创作节点。只支持就绪的图片、视频、GLB、Word/PDF；不上传文件、不触发生成。未指定布局由后端决定；显式尺寸需宽高同时提供，位置为有限数、尺寸为正。

CLI 固定第一次解析的来源与创建请求；结果不明沿用原请求键，不因原资源改名重新选另一项。后端可能为画布绑定独立资产，后续用响应 `element.content_ref.id`，不固守来源 asset ID。

## 排列选定节点

```bash
virse canvas layout --element "$FIRST" --element "$SECOND" --mode grid --columns 2 --gap 40 --x 100 --y 200 --idempotency-key "$KEY" --org "$ORG" --canvas "$CANVAS" --json
virse canvas layout --group "$GROUP" --mode row --gap 40 --org "$ORG" --canvas "$CANVAS" --json
```

可选 row、column 或 grid，按输入顺序和当前节点宽高计算位置。节点集合最多 100 项，组合最多 99 个成员；组合排版同事务更新其边界。布局保留内容并核对各节点布局修订，间距非负。它不是图内排版、图片重构或 3D 姿态修改。

交付时检查文字是否完整可读、相邻对象是否遮挡。保存全文或几何无重叠不能代替实际可见效果；有前端查看条件时检查真实显示，没有查看条件时说明未验证。CLI 没有文本测量、画布截图或只读预览入口。

## 复制到当前或另一画布

```bash
virse canvas copy --element "$FIRST" --element "$SECOND" --dx 80 --dy 80 --idempotency-key "$KEY" --org "$ORG" --canvas "$CANVAS" --json
virse canvas copy --group "$GROUP" --to-canvas "$TARGET_CANVAS" --idempotency-key "$KEY" --org "$ORG" --canvas "$CANVAS" --json
```

最多选择 100 个节点。复制生成新身份，保留可支持的节点内容，重建选中范围内部连线与可选组合，并重映射内部生成配置引用；外部依赖跳过，不把所有上下游自动复制。目标画布仍需当前身份有权写入。

可编辑 MarkSpace 文档不支持该复制协议，应明确此边界；用户需要静态图时可另行取得其已保存图片，不能声称保留可编辑结构。结果不明保持原选择、偏移、目标和键重放冻结计划，不生成第二批 ID。

## 核对保存结果

用返回的新身份与 `element/group/edge get/list` 检查位置、组合成员和内部连线。不要因为复制成功就声称引用的外部材料、运行状态或完整历史都一并迁移。需要交付媒体文件时直接读[文件交付](shared/contracts/delivery.md)，不把画布结构复制当成 ZIP 工程导出。
