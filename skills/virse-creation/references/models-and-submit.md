# 模型与提交方式

## 选择当前模型

```bash
virse --org "$ORG" --json model list --mode image
virse --org "$ORG" --json model get "$MODEL"
```

已知 model_key 时先 `model get` 精确查询；需要选型才列目录。`--mode` 可按需要选择 `image/video/text/3d`，可重复指定；只查当前任务需要的范围。用户有偏好时 `--favorites` 看本人收藏；解释暂不可用原因时加 `--include-disabled`，而不是把目录中出现过的模型一律视作可调用。

get 按准确 model_key 返回所有匹配条目，同一 key 可对应不同 mode。image 目录也包含专用图片处理条目，结合当前返回的 mode、capabilities、supported_scenarios、资格与限制选择，不把 image 条目全当文生图模型；扩展字段不保证齐全。目录输出过大时，将完整 JSON 保存到本地再按当前响应结构筛选所需字段，不为摘要重复读取全目录，也不虚构服务端筛选或字段投影 flags。

需要解释默认模型或价格时，保留顶层 defaults、条目 default_for、默认参数与各价格项的原值；字段冲突或组合价选择顺序不明时说明差异，不自行指定优先级。目录价格不是实际扣费回执或锁价，price、unit_prices.default 等价格字段不能凭名称解释为本次费用。pricing_by_item 中出现某操作的价格，也不证明该操作已开放或参数有效；提交仍以操作输入合同和模型能力为准。

## 管理模型收藏

```bash
virse --org "$ORG" --json model favorite "$MODEL"
virse --org "$ORG" --json model unfavorite "$MODEL"
```

两条按用户目标选一条。核对返回 `is_favorite`，受理未知时先 get/list。收藏只改个人偏好，不开放权限、启用不可用模型或改变其他人的收藏。

## 选择快捷命令或通用请求

普通图片、视频、3D、文本和图片处理优先使用对应快捷命令；模型参数采用本次真实合同。图片、视频、3D 的 `--parameters` 与 `--parameters-file` 互斥，内容须为 JSON 对象；不要将画布显示尺寸当作模型分辨率。

当快捷命令不能表达所选操作或多输出目标时：

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation operations
virse --org "$ORG" --canvas "$CANVAS" --json generation submit --file request.json --idempotency-key "$KEY" --wait
```

按当前 operation 的真实输入 Schema 填 input，不从功能名猜字段。通用请求的 envelope、输出约束、参考与受理语义见[生成任务合同](shared/contracts/generation.md)。operation 目录不是全站 Schema，也不能证明未声明的 3D 工作台或其他产品功能已开放。

## 多输出与目标绑定

需要多个候选时先确认当前 operation 支持多个 outputs，再明确每个 output_index 对应哪个新节点或已有目标。通用上限 100 不是所有模型都能一次生成 100 个结果；媒体快捷命令单次各提交一个输出。

对已有节点使用目标身份，对新节点提供显示位置和尺寸；两者按项互斥。保留输出索引与节点、资产映射，完成后逐输出核对，不用任务整体状态掩盖部分失败。若用户只说“多做几个”，按其目标选择可用的多输出请求或分别提交明确请求，不虚构快捷命令的批次数参数。
