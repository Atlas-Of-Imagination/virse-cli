# 图片创作与处理

## 生成图片

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation image --model "$MODEL" --prompt "$PROMPT" --idempotency-key "$KEY" --wait
```

有明确参考时逐个增加 `--reference "$ELEMENT"`，按所需顺序传入；需要已核清的模型参数时使用 `--parameters-file parameters.json`。模型与 image_generate 的输入能力来自当前查询，不写死模型名称、参考上限或价格。

新输出可指定 `--x/--y/--width/--height`；原节点继续生成用 `--target-element "$TARGET"`，两类参数互斥。本次仍须提供明确 prompt 与模型。完成后关联 task、节点、资产并查看实际图片。

## 按指令编辑图片

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation edit --source-element "$SOURCE" --model "$MODEL" --prompt "$EDIT" --idempotency-key "$KEY" --wait
```

来源也可用 `--source-asset "$ASSET"`，与 source-element 二选一；asset ID 不是目录资源项 ID。模型须支持当前图像编辑场景。主图固定为 `$1`，额外 `--reference` 从 `$2` 开始，总量仍受模型限制。

编辑来源为节点时，CLI 冻结其当前资产身份。未指定原目标或新布局时，通常在源图右侧新建结果并沿用显示尺寸；显式输出目标以用户意图为准。无蒙版编辑不能承诺只改变精确区域，执行后仍需检查保留项。

## 蒙版局部重绘

模型支持 inpaint 且已有准备好的 PNG 蒙版时，在 edit 上增加 `--mask-file mask.png`，或使用有下载权限的 `--mask-asset "$MASK"`；两者互斥。白色表示编辑、黑色表示保留，蒙版按源图像素尺寸准备且不超过 50 MiB。CLI 不提供选区画笔或自动制作蒙版的工作台。

CLI 先校验并上传蒙版，取得确定响应后才提交生成。出现 `MASK_UPLOAD_UNCERTAIN` 时没有提交生成，上传又没有可查询回执；不要自动重传或换键重复尝试。普通生成已受理但响应未知才按[生成任务合同](shared/contracts/generation.md)恢复。黑区表示保留约束，不保证结果与源图逐像素相同。对照实际源图与结果检查白区编辑和黑区保留；要求严格保留时，在尺寸与坐标一致的前提下比较黑区像素差异，分别说明差异范围与幅度，不把目视相近写成完全不变。

## 去背景与放大

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation remove-bg --source-element "$SOURCE" --model "$MODEL" --idempotency-key "$KEY" --wait
virse --org "$ORG" --canvas "$CANVAS" --json generation upscale --source-element "$SOURCE" --model "$MODEL" --scale "$SCALE" --idempotency-key "$UPSCALE_KEY" --wait
```

按任务选用命令，来源也可换成互斥的 `--source-asset`。scale 为正整数，模型实际支持的倍率、输出分辨率和效果由当前合同决定。检查去背景的主体保留与边缘、放大的细节及是否引入伪影；不因任务成功就认定透明通道或细节质量达到要求。

## 按已准备的扩展蒙版扩图

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation outpaint --mask-element "$MASK_ELEMENT" --model "$MODEL" --prompt "$PROMPT" --idempotency-key "$KEY" --wait
```

也可用互斥的 `--mask-asset "$MASK_ASSET"`。mask 是已经准备好、可授权引用的扩展蒙版图片；需要原图等其他参考时按当前 operation 合同提供。此命令不负责交互扩大画板或现场绘制区域。扩展蒙版的像素布局、透明度及原图嵌入方式须来自当前消费合同或已确认素材要求，不能套用 inpaint 的黑白规则或自行假定 RGBA 格式。检查扩展接缝、主体保留与新内容的合理性，不能把输出画布节点变大等同于图像已经扩展。

若运行时返回 422、未知参数等合同不一致错误，记录真实错误、明确指出的字段及是否取得任务身份，停止该路径；错误未指出字段时不猜具体原因。可只读核对当前命令 help、operation 输入合同和已有任务，不猜私有后端字段、服务器文件路径或反复改请求提交。此次拒绝不等于扩图永久不可用；蒙版上传成功也不等于扩图已受理或完成。
