# 文件交付

## 选择交付对象

按用户完整请求列清交付范围，包括文件、画布内容、说明或其他明确要求；不能用一张预览或单个成功文件代替全部交付。已有资源路径或资源项 ID 优先走 `resource` 入口；画布任务返回的资产先核对真实 `content_ref.id`，再按画布上下文访问。资源项 ID、asset ID 和画布空间 ID 不能互换，见[对象关系](../context/objects.md)。先分清用户需要查看展示、下载原件，还是打包多份文件；这些操作不重新生成内容或转换格式。

## 取得授权预览或展示文件

查看缩略图、封面或已授权的展示内容时：

```bash
virse resource preview "$RESOURCE" --representation preview --output ./preview.bin --org "$ORG" --json
virse asset preview "$ASSET" --representation content --output ./display.bin --org "$ORG" --canvas "$CANVAS" --json
```

`preview/content` 是服务端展示表示的选择，不是提升权限的方法。取得预览不代表拥有原件下载资格，缺少封面或表示时不回退原件；不能拿预览下载结果冒充原文件。服务端返回的实际格式决定如何查看，输出扩展名不转换字节格式。输出文件必须不存在。

## 下载原件

```bash
virse resource download '/项目素材/产品.png' --output ./product.png --org "$ORG" --json
virse asset download "$ASSET" --output ./product.png --org "$ORG" --canvas "$CANVAS" --json
```

资源入口不需要猜 asset 或画布 ID；已知资产原件入口使用对应组织与授权画布。CLI 经授权取得字节，保存临时文件，按可用的大小、内容修订和 SHA-256 检查后发布最终文件，不覆盖已有目标。没有校验和时保持未知，不编造验证值。

交付时核对真实本地路径、字节数、内容身份和可用版本信息。保存成功、任务终态、内容正确和视觉达标分别验收。查看实际文件，不能只凭扩展名认定是有效图片、视频或模型。权限失败不绕过为桶路径、预览 URL 或其他身份。

## 断点续传

大文件需要接续时，从第一次下载就使用 `--resume`：

```bash
virse resource download "$RESOURCE" --output ./product.glb --resume --org "$ORG" --json
virse asset download "$ASSET" --output ./product.glb --resume --org "$ORG" --canvas "$CANVAS" --json
```

CLI 保存 `.virse-part` 部分文件及检查点；恢复使用原身份、输出位置和服务端内容版本。需要版本化内容元数据和 `source_version`，已有部分字节还需可核验的对象版本、校验和或强 ETag 等依据；不满足时报告不支持接续。身份或内容变化、部分文件被修改时拒绝拼接，选择新的目标路径，不强行追加 HTTP 200 响应。已有最终文件不覆盖。

## 资源文件 ZIP

```bash
virse resource pack "$FIRST_RESOURCE" "$SECOND_RESOURCE" --output ./delivery.zip --org "$ORG" --json
```

每次选择 1–200 个资源文件，逐个授权下载原件，ZIP 包含字节文件和 `manifest.json`。它不递归导出目录或整个工程，不负责格式转换。输出 ZIP 必须不存在。

## 画布媒体 ZIP

```bash
virse canvas pack --element "$FIRST_ELEMENT" --element "$SECOND_ELEMENT" --output ./canvas-media.zip --canvas "$CANVAS" --org "$ORG" --json
virse canvas pack --group "$GROUP" --output ./group-media.zip --canvas "$CANVAS" --org "$ORG" --json
```

单次选择节点或一个组合，最多 200 个实际成员。只交付绑定了 asset 的 image/video/3d/document 字节；内联文本、可编辑画板等无可下载媒体的节点列为跳过。组合按成员交付，不合成图片；不导出讨论、布局工程、图层工程、PSD 或相机渲染结果。

## 核对逐项结果

打包回执和包内清单记录来源 resource/asset/element、`outcome`、文件名、字节数及可用版本；失败有 error，跳过有 reason。逐项确认 `succeeded/failed/skipped` 是否符合用户交付范围。存在失败或没有成功文件时退出 6；即使退出 0 也可能有跳过项，不能把 ZIP 存在当成全部交付。

本地 Word 成品可交给当前环境已安装的文档制作 Skill，并检查中文字体与实际渲染；这不代表 Virse 原生支持 Word 编辑，也不新增无关依赖。CU 或其他成本验收由执行任务的上层工作流依据操作归因核对，不是文件交付命令的能力；没有操作级归因时，不能从多人共享余额差推算本次 CU。

报告已交付文件和缺少范围。要重试失败项时只选择确认需要的对象，使用新的本地输出路径，避免重做生成或覆盖已有交付。一般输出与错误约定见[执行与恢复](execution.md)。
