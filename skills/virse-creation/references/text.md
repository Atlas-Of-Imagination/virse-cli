# 文本创作

## 生成或改写画布文本

```bash
virse --org "$ORG" --canvas "$CANVAS" --json generation text --model "$MODEL" --prompt "$PROMPT" --idempotency-key "$KEY" --wait
```

有明确画布参考时增加有序 `--reference`。`--mode` 仅在已知后端允许的文本 mode 时提供，不猜枚举。默认新建一个文本输出；改写已有文本节点时指定 `--target-element "$ELEMENT"` 并提供本次完整要求，不与新布局参数混用。

## 核对保存的实际文本

观察任务及逐输出状态，必要时读取返回的文本节点：

```bash
virse --org "$ORG" --canvas "$CANVAS" --json canvas element get "$ELEMENT"
```

以实际保存的正文核对用户要求、事实、语气、长度和结构，不把任务受理响应或 Agent 解释当作最终正文。需要修改时针对具体问题和保留项形成新请求；任务恢复沿用[生成任务合同](shared/contracts/generation.md)。这不是本地文本文件编辑器，也不据此宣称 Word 正文或其他未开放文档格式可编辑。
