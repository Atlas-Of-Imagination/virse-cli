---
name: virse-audio-craft
description: >-
  音频创意与提示词，选择语音、对白、换声、音乐、音效与音频处理的任务模式，挑选音色，编写台词、歌词和声音描述，处理多轨与转写产物并听音诊断。可只交付台词或描述；实际生成配合 virse-creation，状态查询与资产管理走业务入口。
---

# 音频创作技巧

<!-- virse:background:start -->
Virse 是 AI native 的创作系统，用画布连接想法、素材、审美、工具与模型，支持图片、视频、音频、3D 和文本。资源目录管理素材与归属，画布组织创作输入、过程与结果。

只读当前任务所需参考，已读且适用的内容沿用，链接不自动加载内容。
<!-- virse:background:end -->

## 先确定要固定什么

作为与用户共同推进声音作品的创作家，结合已有且获准使用的素材与上下文，判断用途、内容、必须保留的声音特征和可自由发挥的部分。信息足够就推进，只补问会改变任务模式的缺口，例如“这段录音要保留原来的说话内容只换音色，还是只借它的声音特征重新念一段新台词”。不把完整访谈作为写台词或描述的前置。

用户只要台词、歌词、声音描述或诊断，就交付相应内容，不提交生成。实际在 Virse 执行时，按名称读取 `virse-creation` 及其 `references/audio.md`，查真实模型能力、任务模式、输入槽位和参数，再按已有授权范围执行。创作偏好不等于长期自主生成授权。给视频配声且需要设计画面动作或判断视频自带声音时，才按名称读取 `virse-video-craft`；不将视频技巧设为所有音频任务的前置。

需要其他 Skill 时先确认宿主能够读取；未安装就说明缺少哪个包并帮助定位安装方式，不假设名称已带入其内容。台词、描述与听音诊断仍可在本 Skill 内完成。

## 任务模式

音频只有一个 `audio` 模式，任务由 `parameters.audio_mode` 决定且必填。下表按分类列出 Virse 的任务模式；分类只用于挑选，不提交。

| 分类 | `audio_mode` | 什么时候选 | 输入固定什么，文字补什么 | 需要时读取 |
| --- | --- | --- | --- | --- |
| 语音 | `tts` | 一个人念一段台词或旁白 | 台词就是要念的文字；音色来自预置音色或模型声明的声音参考 | [提示词](references/prompt-craft.md)、[音色](references/voices.md) |
| 语音 | `dialogue` | 两个人交替说话 | 台词逐行标说话人；两个音色按说话人顺序给出 | [提示词](references/prompt-craft.md)、[音色](references/voices.md) |
| 语音 | `voice_change` | 已有录音，保留说话内容与语气，只换音色 | 源音频提供内容与节奏；不写台词，选目标音色 | [音色](references/voices.md)、[素材与参考](references/sources-and-references.md) |
| 音乐 | `music_instrumental` | 不要人声的音乐 | 描述曲风、乐器、速度感、情绪与段落起落 | [提示词](references/prompt-craft.md) |
| 音乐 | `music_auto_lyrics` | 要有人声的歌，歌词交给模型 | 描述主题、风格与人声；不写完整歌词 | [提示词](references/prompt-craft.md) |
| 音乐 | `music_custom_lyrics` | 用户已有歌词 | 歌词放进歌词参数，描述只写曲风与演唱 | [提示词](references/prompt-craft.md)、[参数](references/parameters.md) |
| 音乐 | `music_reference` | 借已有音乐的曲风、旋律或人声生成新歌 | 每份参考写明它在哪个槽位、借什么 | [素材与参考](references/sources-and-references.md) |
| 音乐 | `music_cover` | 以一首已有歌曲为底改编 | 源音频提供原曲；文字说改成什么样 | [素材与参考](references/sources-and-references.md) |
| 音乐 | `video_score` | 给已有视频配音乐 | 源视频提供时长与画面节奏；文字描述音乐 | [素材与参考](references/sources-and-references.md) |
| 音乐 | `music_inpaint` | 只重做音乐里的一段 | 源音频与时间区间固定上下文；文字写那一段要什么 | [参数](references/parameters.md) |
| 音乐 | `music_extend` | 在已有音乐前后接着生成 | 源音频提供起点；文字写接下来的发展 | [素材与参考](references/sources-and-references.md)、[参数](references/parameters.md) |
| 音效 | `sfx` | 没有素材，凭描述生成音效或环境声 | 描述声源、动作、空间与时长 | [提示词](references/prompt-craft.md) |
| 音效 | `video_foley` | 给视频里的动作配事件声 | 源视频提供动作与时长；按需选择对白如何处理 | [素材与参考](references/sources-and-references.md)、[多轨与转写](references/tracks-and-transcripts.md) |
| 音效 | `sfx_inpaint` | 只重做音效里的一段 | 源音频与时间区间；文字写那一段 | [参数](references/parameters.md) |
| 音效 | `sfx_variation` | 已有音效，要做相近的变体 | 源音频提供基准；文字写要变化的方向 | [素材与参考](references/sources-and-references.md) |
| 处理 | `transcribe` | 把录音转成文字 | 源音频；产物是文本节点 | [多轨与转写](references/tracks-and-transcripts.md) |
| 处理 | `vocal_isolation` | 去掉伴奏或背景噪音，只留人声 | 源音频或源视频（按模型声明二选一） | [多轨与转写](references/tracks-and-transcripts.md) |
| 处理 | `stem_split` | 把一首歌拆成人声、鼓、贝斯等分轨 | 源音频；按目录声明的档位决定轨数 | [多轨与转写](references/tracks-and-transcripts.md) |
| 处理 | `sound_separation` | 从混音里分离出描述的某种声音 | 源音频；文字写要分离的声音，得到目标与其余两轨 | [多轨与转写](references/tracks-and-transcripts.md) |

这些是 Virse 的任务入口，不是所有模型都支持的能力清单。所选模型 `generation_capabilities.audio_modes` 是否声明该任务，以及该任务的槽位、必需输入、参数和输出轨道，决定能否提交及如何绑定；不把任务名直接当成 CLI flag。

## 按问题取用方法

只读相关参考。需要核实模型特有方法时，再查该模型的官方说明。

| 当前要解决的问题 | 读取 |
| --- | --- |
| 写台词、对白、歌词或声音描述，消除歧义与堆叠 | [提示词](references/prompt-craft.md) |
| 选预置音色、双人对白配音色、用声音参考或换声 | [音色](references/voices.md) |
| 源音频、参考音频、参考图或源视频各起什么作用，槽位与时长限制 | [素材与参考](references/sources-and-references.md) |
| 选时长、次数、区间、歌词、档位等参数及组合限制 | [参数](references/parameters.md) |
| 一次产出多条轨道、多份结果，或转写为文本 | [多轨与转写](references/tracks-and-transcripts.md) |
| 听结果、定位问题、比较并决定下一次改什么 | [查看与迭代](references/review-and-iterate.md) |

实际生成后听真实音频，按问题选择完整播放、重听局部和对照原素材。任务成功、文件存在、波形和时长正常都不能证明内容与声音已验收；未能听到的部分要明确说明。
