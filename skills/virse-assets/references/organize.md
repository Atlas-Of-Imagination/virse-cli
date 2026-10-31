# 整理与归档

## 创建与改名

在已有可写父目录下创建单层文件夹：

```bash
virse resource mkdir '交付' --parent '/项目素材' --org "$ORG" --json
virse resource rename "$RESOURCE" '产品主图.png' --idempotency-key "$KEY" --org "$ORG" --json
```

也可对 `mkdir` 使用完整新路径，但不会递归创建缺少的父目录。建目录没有幂等回执；结果不明先列父目录对账，不能自动重建。改名使用原请求键恢复，保持第一次解析的资源身份，不重新按变化后的路径选别的对象。

## 移动与复制

```bash
virse resource move "$RESOURCE" '/项目素材/交付' --idempotency-key "$KEY" --org "$ORG" --json
virse resource copy "$RESOURCE" '/项目素材/备份' --idempotency-key "$KEY" --org "$ORG" --json
```

最后一个位置参数必须是已有目标文件夹或 `/`，不是新文件名；可在前面提供多个来源。移动保留资源身份，复制创建独立资源。两者在同一组织上下文内执行，不提供跨组织原件移动，不隐式覆盖或合并。

移动若返回 `visibility_widened/visibility_narrowed`，先检查服务端影响；用户授权已包含该访问变化时，使用 `--confirm-access-change` 和新请求键只重做失败项。未包含时说明实际变化并取得授权。不能重做已成功项，未知结果则沿用原内容和原键核对。访问规则的实际更改按分享职责处理。

## 置顶收藏与排序

```bash
virse resource pin "$RESOURCE" --org "$ORG" --json
virse resource unpin "$RESOURCE" --org "$ORG" --json
virse resource star "$RESOURCE" --org "$ORG" --json
virse resource unstar "$RESOURCE" --org "$ORG" --json
virse resource favorites --org "$ORG" --json
virse resource reorder '/项目素材' "$FIRST" "$SECOND" --org "$ORG" --json
```

置顶是 `pinned`，本人收藏是 `quick_access`；它们不移动资源或授予访问权。快捷访问结果可能含工具入口，按真实类型处理。自定义顺序每次最多 500 项，服务端校验目录成员与权限。标记或排序结果不明时先重读，不凭网络错误推断未写入。

## 移除外部入口

```bash
virse resource hide-shared "$RESOURCE" --org "$ORG" --json
```

只从本人的外部共享列表移除入口，不删除原件、不撤销规则，也不影响他人的入口。结果不明先查 `ls / --view external`。

## 将画布媒体另存为资源

已有画布媒体需要归档或复用时：

```bash
virse resource save-canvas --element "$ELEMENT" --parent '/项目素材/交付' --idempotency-key "$KEY" --canvas "$CANVAS" --org "$ORG" --json
virse resource save-canvas --group "$GROUP" --parent '/项目素材/交付' --idempotency-key "$KEY" --canvas "$CANVAS" --org "$ORG" --json
```

可重复 `--element`，或选择一个组合，单次 1–200 个实际成员。首次固定成员和目标目录，逐项返回 `succeeded/failed/unknown` 及资源身份。组合不会合成为一张图；没有可另存字节的节点不能伪造成文件。另存保留画布原内容，结果不明按原选择与原键接续。下载与 ZIP 另见[文件交付](shared/contracts/delivery.md)。
