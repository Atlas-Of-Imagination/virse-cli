# 生成任务合同

## 构造与提交

已知模式优先使用 `generation image/video/3d/text/edit/remove-bg/upscale/outpaint`。需要特殊 operation、输入或多个输出时，先取目标画布当前操作目录，再按其中真实的输入合同准备 JSON：

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation operations
virse --org "$ORG" --canvas "$CANVAS" --json generation submit --file request.json --idempotency-key "$KEY" --wait --timeout 600
```

`--file -` 从 stdin 读取。请求不超过 10 MiB，顶层只有 `operation`、对象 `input`、数组 `outputs`；通用 submit 不是任意 HTTP 或全站 Schema 入口。保留完整 request.json，参数与 operation 以本次目录为准，不把另一个画布或后端的合同直接套用。

每项输出使用 `output_index`，并且恰好选择 `element_id` 或 `create_element`。后者包含 `position:{x,y}`、`size:{width,height}`，可附新节点 `id/title`。索引不重复且在输出数组范围内，目标 ID 不冲突，尺寸为正有限数。通用校验允许 1–100 个输出，实际 operation 是否支持及支持几个由运行时决定；媒体快捷命令各只提交一个输出。

## 输出位置与参考

快捷命令用 `--target-element "$ELEMENT"` 将本次结果写入已有匹配类型节点，不能同时给新节点坐标尺寸。它不恢复原 prompt、模型或参数；本次输入仍须明确提供。新建输出可用 `--x/--y/--width/--height`，仅控制画布显示位置与尺寸，不控制图内主体或模型输出分辨率。

支持引用的快捷命令按重复 `--reference "$ELEMENT"` 的顺序绑定 `$1`、`$2` 等。图片 edit 将主图固定为第一参考，额外参考从 `$2` 开始。总参考上限 100，模型可更少；通用请求按所选 operation 的真实参考结构填写，不能把快捷 flag 机械翻成任意 JSON 字段。参考顺序不能保证构图、角色或多视图一致。

## 定位与逐输出结果

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation list --element "$ELEMENT" --offset 0 --limit 50
virse --org "$ORG" --canvas "$CANVAS" --json generation get "$TASK"
```

列表可按 `--id/--element/--artifact-version/--status/--operation` 筛选，offset 分页每页 1–100。请求全集时保持同一筛选续页，不能把第一页当全部历史。节点当前 `generation.config` 与任务查询都不保证完整历史 prompt、原始参考、每次参数或供应商最终 prompt。

任务核对 `task_id/project_id/operation/status/revision/actions`；逐输出核对 `output_index/element_id/status/asset_id/error_code`。整体状态和输出成败分别解释，部分成功保留成功项。`get` 退出 0 只证明读取成功；资产身份不证明文件已下载，也不证明作品质量。

## 等待与控制

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation watch "$TASK" --timeout 600
virse --org "$ORG" --canvas "$CANVAS" --json generation cancel "$TASK"
virse --org "$ORG" --canvas "$CANVAS" --json generation retry "$TASK" --idempotency-key "$RETRY_KEY" --wait
```

watch 先读当前任务再观察 WebSocket 状态快照，CLI 负责修订过滤和重连校准；这是当前状态观察，不是事件历史。JSON watch 输出 `task_snapshot`。任务 `succeeded` 退出 0，`failed/partially_succeeded/cancelled` 退出 6，`needs_attention` 退出 7；等待超时退出 8，Ctrl+C 退出 130。超时、断线和本机中断不会取消远端任务，不再新建轮询或重投循环。

先读取 `actions` 与当前状态，再按服务端许可取消或重试。取消调用成功不等于任务已 cancelled，运行中可能拒绝取消。retry 由服务端选择可重试失败输出，创建关联的新任务；记录新 task ID 及实际返回的 retry_of 信息，不假定成功输出全部重跑。retry 的受理未知时沿用原任务 ID 和同一 `RETRY_KEY` 查证，不另换键重复请求。

## 恢复未知提交

普通生成提交返回 `SUBMISSION_UNCERTAIN` 时，保持原后端、身份、画布、完整输入及原 `--idempotency-key`，重放原提交；已取得 task ID 就继续观察该任务。CLI 只保存普通提交的请求摘要和任务身份，不保存完整输入，不能从回执自动重建丢失的 prompt 或参数。

改变 prompt、参数、参考、输出或目标属于新创作，使用新键。图片 edit 的 `MASK_UPLOAD_UNCERTAIN` 是不同阶段：尚未提交生成，蒙版上传无可查询回执；不自动重传蒙版或换新键猜测执行。先查明已有结果，明确新尝试后再处理。

## 完成与交付

按输出关联实际节点与内容，再依[文件交付](delivery.md)取得获准预览或原件。对审美测试，另记录对应草稿 revision、测试快照与本次任务输入；本合同不定义审美 action body，也不凭空增加消费方审美字段。

提交受理、任务终态、内容保存、文件取得与内容质量分别判断；逐项核对用户完整要求，不用成功状态替代内容验收。视觉要求需要查看真实产物；没有适用查看能力时说明验证边界。
