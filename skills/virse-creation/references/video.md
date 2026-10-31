# 视频创作

## 从目标与参考生成视频

先明确视频用途、关键内容、要保持的主体或风格，以及用户已有素材。时长、输入参考角色、分辨率、音频和其他能力以所选模型与当前 operation 为准；不为每种模型假设相同的首尾帧或视频参考字段。

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation video --model "$MODEL" --prompt "$PROMPT" --parameters-file parameters.json --idempotency-key "$KEY" --wait
```

没有额外参数时省略 parameters-file。参考节点按真实允许的数量和顺序增加 `--reference`。快捷命令提交一个 video_generate 输出；特殊角色或多个输出用运行时已声明的通用操作，提交方式见[生成任务合同](shared/contracts/generation.md)。

## 检查与继续修改

关联 task、每个输出和实际视频资产，取得有权查看的[预览或原件](shared/contracts/delivery.md)。使用宿主实际支持的视频查看能力核对关键内容、时间变化和连贯性；静态封面只能证明封面可见，不能代表已检查整段视频、声音或所有帧。

根据具体问题调整这次 prompt、参考或支持的参数，保留基准结果与输入关联。快捷命令不提供独立时间线编辑器；未在 operation 声明的视频处理不承诺可用。不能把重新生成视频称为对原片精确逐帧编辑。
