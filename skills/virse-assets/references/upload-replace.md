# 上传与替换

## 上传一个文件

新增材料时先确定已有目标目录与同名策略：

```bash
virse resource upload ./product.png '/项目素材' --conflict rename --idempotency-key "$KEY" --org "$ORG" --json
```

支持图片、视频、DOCX、PDF、GLB，具体格式与限制见[导入批次](import-batches.md)。JSON、TXT、SVG 不作为已支持上传格式；需要转换时保留原件和来源关系，转换后核对实际格式与内容。单个普通文件上限 100 MiB，实际格式由服务端检查，不靠扩展名转换。`rename` 为同名另起名，`skip` 只表示跳过，不证明远端字节相同；`error` 遇到冲突失败。保留返回的真实资源项、asset ID、名称和内容修订，不猜服务器最后选中的文件名。

上传不自动放入画布或创建生成任务。整目录或多个文件需要持久进度时用[导入批次](import-batches.md)。

## 保留资产身份替换原件

先读取目标资源，再取返回资产中的实际 `content.revision`：

```bash
virse resource get "$RESOURCE" --org "$ORG" --json
virse resource replace "$RESOURCE" ./product-new.png --expected-revision "$CONTENT_REVISION" --idempotency-key "$KEY" --org "$ORG" --json
```

替换后重新读取资源及相关画布节点的 `content_ref.id`，区分原资产、画布引用和独立副本；不假定同名文件或画布副本自动更新。对照替换回执、实际 asset ID、内容修订及可用校验和，必要时下载实际字节核验，不能只看缩略图或名称宣布同步。

替换保留 asset ID，要求同类文件；Word/PDF 不能互换。内容修订必须显式给出，冲突先比较最新内容，不读取新修订后强制覆盖。GLB 需要清除既有编辑状态时，另提供 `--reset-model-state --expected-state-revision`；模型状态修订与内容修订互不替代，这不等于 3D 工作台保存。

## 恢复原请求

CLI 在提交前保存原始字节、身份、目标和请求。普通重试检查本地文件未变，并沿用相同 `--idempotency-key`；原文件已移动、变化或删除时，明确使用保存快照：

```bash
virse resource upload ./product.png '/项目素材' --idempotency-key "$KEY" --resume --org "$ORG" --json
virse resource replace "$RESOURCE" ./product-new.png --expected-revision "$CONTENT_REVISION" --idempotency-key "$KEY" --resume --org "$ORG" --json
```

恢复要求已有记录和原键，使用冻结的来源、字节与目标，不重新解析已改名路径。已接受后文件又被修改时，保留 `content_changed/revision_conflict` 和完成版本信息，不返回过期缓存成功。未知结果保留原键对账，不换键盲目上传第二份，也不删除可能已成功的对象。具体错误结构见[执行与恢复](shared/contracts/execution.md)。
