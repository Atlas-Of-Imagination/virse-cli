# 音频创作

需要判断语音、对白、换声、音乐、音效或音频处理中的哪种任务模式，选音色，写台词、歌词与声音描述，或处理多轨与转写产物时，读 `virse-audio-craft` 并只加载当前任务需要的方法。先分清素材是被处理的源音频、声音参考还是画面参考，再按运行时合同绑定输入；在台词里写说话人名字不替代音色选择。

## 从目标与素材生成音频

先明确音频用途、内容、要保留的声音或节奏，以及用户已有素材。音频只有一个 `audio` 模式，具体任务由 `parameters.audio_mode` 决定，必填，取所选模型 `generation_capabilities.audio_modes` 已声明的任务。该任务生效的输入槽位、必需输入、参数白名单、按任务覆盖与输出轨道都在同一处；时长、音色数量、素材时长上下限等以所选模型与当前任务为准，不为每个模型假设相同的参数。

```bash
virse --org "$ORG" --json model list --mode audio
virse --json voices list --model "$MODEL"
virse --org "$ORG" --canvas "$CANVAS" --json generation audio --model "$MODEL" --prompt "$PROMPT" --parameters-file parameters.json --idempotency-key "$KEY" --wait
```

`parameters.json` 至少包含 `audio_mode`，例如单人配音 `{"audio_mode":"tts","voice":["$VOICE"]}`。音色参数（目录类型 `voice`）恒为音色 id 数组，长度等于该任务音色参数定义的 `count`（合并 `param_overrides` 后，如对白为 2；与 `--count` / `audio_count` 无关）；预置音色用 `voices list` 查，模型不带音色参数时该命令报 404，返回空列表表示尚未整理、该模型暂不能使用音色参数，不能编造 id。没有台词或描述的任务（换声、分轨、提取人声等）省略 `--prompt`；任务把 `prompt` 列为必填时才写。

`--count` 是生成次数（请求的 `audio_count`，默认 1），不要写进 parameters。参考节点按当前任务的槽位与真实允许的数量、顺序增加 `--reference`，图片、视频、音频节点都可以引用。同一类型素材在该任务里有多个槽位（例如曲风参考、旋律参考、音色参考都是音频）时，每个引用要写明 `slot`，快捷命令表达不了，改用通用请求；模型级 `input_exclusions` 与参数的 `excludes_inputs` 声明了不能同时提供的输入。提交方式见[生成任务合同](shared/contracts/generation.md)。

## 多轨与转写

一次生成的输出数 = `audio_count` × 该任务的轨数。轨道取 `audio_modes[任务].outputs.tracks`；`outputs.tracks_by` 指定的参数（如分轨的轨数档位）取到声明值时改用对应那组。`generation audio` 从模型目录算出轨数，按 `--count` × 轨数逐个新建输出节点，未给坐标时自上而下排列；分轨等多轨任务一次提交即可，不要用重复提交凑轨道。`output_index` 先按生成次序、再按轨道顺序排列。`--target-element` 只用于恰好一个输出的任务；需要逐轨指定位置或写入多个已有节点时，按[生成任务合同](shared/contracts/generation.md)用 `generation submit` 列全输出，数量不对服务端以 `GENERATION_OUTPUT_COUNT_MISMATCH` 拒绝。

`outputs.kind` 为 `text` 的任务（转写）产出文本节点，不产出音频文件；写入已有节点时目标须是 text 节点。其余任务产出 audio 节点。

## 检查与继续修改

关联 task、每个输出与其轨道、实际音频资产，取得有权查看的[预览或原件](shared/contracts/delivery.md)。用宿主实际支持的播放或音频理解能力听取完整内容，核对台词、说话人、音色、节奏、起止与音量；波形、时长和文件存在只能证明有内容，不能代表已听过。转写结果读取文本节点，并与原音频抽查对照。

根据具体问题调整这次台词、描述、音色、参考或支持的参数，保留基准结果与输入关联。未在任务中声明的剪辑、混音、拼接或格式转换不承诺可用；区间重做、续写只改目录声明的那段或那种处理，不能称为对原音频逐采样编辑。
