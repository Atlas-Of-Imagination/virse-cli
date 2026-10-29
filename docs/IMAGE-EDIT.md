# 图片内容编辑

`virse generation edit` 修改图片内容，复用现有 `image_quick_edit` / `image_inpaint` V2 任务。不改画布布局 API，不需要新增后端接口。

```bash
# 按文字描述编辑，原图固定为 $1；默认在源元素右侧新建结果。
virse generation edit --canvas <canvas-id> --org <org-id> \
  --source-element <image-element-id> --model <image-to-image-model> \
  --prompt '把杯子改成蓝色，其余保持不变' --idempotency-key edit-001 --wait

# 精确提供选区：本地黑白 PNG，白色编辑、黑色保留。
virse generation edit --canvas <canvas-id> --org <org-id> \
  --source-element <image-element-id> --model <inpaint-model> \
  --mask-file ./mask.png --prompt '把选区中的杯子换成玻璃杯' \
  --idempotency-key edit-002 --wait

# 也可引用已有图片资产和蒙版资产；资产 ID 不是资源目录项 ID。
virse generation edit --canvas <canvas-id> --org <org-id> \
  --source-asset <image-asset-id> --mask-asset <mask-asset-id> \
  --model <inpaint-model> --prompt '移除选区中的物体' \
  --idempotency-key edit-003 --x 800 --y 100
```

## 来源、选区和输出

- `--source-element` 与 `--source-asset` 二选一。主图是 `$1`，额外 `--reference` 按输入顺序对应 `$2`、`$3` 等。
- `--mask-file` 与 `--mask-asset` 互斥。蒙版必须为 PNG、最多 50 MiB，建议使用与原图相同的像素尺寸和不透明纯黑/纯白区域；CLI 不做反色、透明度转换、对象分割或选区绘制。完整图片格式校验由上传服务负责。
- 现有局部重绘接口需要上传后的文件引用。CLI 将本地蒙版通过 `/upload_images` 的 `save_asset=false` 上传；已有蒙版资产先按原件下载权限取得字节，再走相同上传。不会从签名 URL 推导桶路径，也不会降级使用预览绕过下载权限。
- 无蒙版是文字指令编辑，范围由模型理解；有蒙版使用真正的 `image_inpaint` 操作。两者均不保证选区外像素完全不变，模型可能调整细节或分辨率。
- 默认新建一个输出元素，保留源图。元素来源沿用其显示尺寸并放到右侧；资产来源默认位置 `(0,0)`、显示尺寸 `1024×1024`。`--x/--y/--width/--height` 可覆盖显示布局，不指定模型输出分辨率。
- 显式 `--target-element` 可把结果绑定到已有图片元素，包括源元素本身；与布局参数互斥。不是原资产文件的原地替换。
- 支持现有 `--parameters` / `--parameters-file`。模型须在当前组织模型目录中支持 `image-to-image` 或 `inpaint`，服务端继续校验具体参数、权限和计费。

## 等待与恢复

- `--wait` 复用 WebSocket 任务观察；超时或 Ctrl+C 只退出等待。可继续 `generation get/watch <task-id>`。
- CLI 将来源、输出位置、参数和成功上传的蒙版引用按账号、后端、组织、画布及幂等键固定。元素来源同时发送 `expected_asset_id`；服务端受理时校验并固定内容。
- 生成提交结果未知时重复原命令和原幂等键，不会重新读取元素位置或再次上传蒙版。同键修改输入或本地蒙版字节会拒绝；本地文件须保留以供重试校验。
- 蒙版上传接口没有幂等回执。上传响应丢失或进程中断会返回 `MASK_UPLOAD_UNCERTAIN`（退出码 7），不提交生成，同键不自动重传。确认情况后可显式换新 key 发起新尝试；这可能留下前一次未确认的中间文件，不声称服务端取消或清理成功。
- 明确的上传权限拒绝保留原 HTTP 错误；修正输入或权限后使用新 key。未上传成功不会发起收费生成。
- 本地日志仅保证当前机器的恢复；不应删除日志后把同一个 key 当作新请求使用。

## 尚未提供

自动对象识别/分割、坐标选区生成蒙版、对象级图层编辑、画板合成和严格的选区外像素保真。

## 验证

2026-10-29：本地 workspace 188 项测试、Clippy 与格式检查通过。dev5 使用独立测试画布，Seedream 4 文字编辑和 Nano Banana 2 本地 PNG 蒙版重绘均生成成功并下载；同 key 重放返回相同任务，源元素内容引用与布局不变。已观察两张结果将红色矩形改为绿色、保留蓝色圆形。已有蒙版资产路径、权限拒绝和丢响应恢复由协议测试覆盖；不代表所有模型或生产环境已验收。
