# 原生包与校验

## 初始化本地工具包

用于从可复用任务建立作者工作区；目标目录必须不存在。

```bash
virse tool init ./product-shot-tool --name '商品图制作'
```

命令离线创建 `SKILL.md`、`interface.json` 与 `meta.json`，不会建立远端工具。根据用户要复用的任务改写初始内容，不保留脚手架占位说明。

工具包根目录只包含一份 `SKILL.md`。说明中写清任务、输入含义、必要步骤、结果和真实限制；需要脚本、参考或素材时才加入，文件与说明相互对应。不要机械补固定章节。

## 编写原生说明与输入

原生包 `SKILL.md` 使用 UTF-8、单行标量 frontmatter；`name`、`description` 和正文非空。CLI 的原生包解析器不支持嵌套或多行 YAML；`when_to_use` 是发现提示，初始化时填写并按任务修订。

```markdown
---
name: 商品图制作
description: 根据商品参考图和用途制作商品展示图。
when_to_use: 需要围绕已有商品图制作新的展示画面时使用。
---

# 商品图制作

保留输入商品的外形、标识与颜色。根据用户用途选择背景和构图，完成后查看实际图片，检查商品一致性并返回结果。
```

`interface.json` 可省略；存在时根对象包含 `fields` 数组，格式版本为 `1`。例如：

```json
{
  "version": 1,
  "fields": [
    {"id": "product", "type": "image", "label": "商品参考图", "required": true, "maxCount": 1},
    {"id": "usage", "type": "text", "label": "用途", "required": true}
  ],
  "promptTemplate": "围绕 {{product}} 制作适合 {{usage}} 的商品展示图。"
}
```

字段 ID 使用字母或下划线开头，后续为字母、数字或下划线，不重复、不使用保留名 `module`。当前本地解析器接受 `text`、`textarea`、`image`、`file`、`style`、`select`、`video`、`multiselect`、`number`、`color`；选择类必须提供非空、不重复的 `options[].value`。正文模板和步骤中的输入引用用 `{{field_id}}`，不把 JavaScript 的 `${field_id}` 当成字段占位。

输入表单描述工具接受什么，不决定每次模型或后端具有什么能力。发布后执行时仍读取固定 artifact 的 Schema，按字段 ID 提供真实资产或画布引用，见[运行合同](shared/contracts/tool-run.md)。

需要声明步骤和预期交付时，在同一 `interface.json` 中增加可选 `steps`、`outputs`，例如：

```json
{
  "version": 1,
  "fields": [{"id": "product", "type": "image", "label": "商品参考图", "required": true}],
  "steps": [{"id": "inspect", "title": "检查商品特征", "body": "查看 {{product}}，确认需要保留的外形与颜色。"}],
  "outputs": [{"type": "image", "label": "商品展示图", "count": 1}]
}
```

`fields` 最多 30 项，`steps` 最多 20 项；步骤 `id` 使用合法且不重复的标识，省略时由序号补成 `step_1` 等。`title/titleEn` 最多 80 字符，`body/bodyEn` 最多 2000 字符，正文中的 `{{field_id}}` 必须来自 fields。outputs 每项有非空字符串 `type`，可选字符串 `label/labelEn` 和正整数 `count`。步骤与输出声明不自动编排调用或保证实际产物；执行逻辑写在工具说明及其实际引用中，运行后核对真实结果。

## 发布元数据

`meta.json` 使用以下本地字段名，JSON 字段不在命令 help 中；不要照搬 HTTP 的 snake_case 字段。

| 字段 | 类型与用途 |
| --- | --- |
| `name`、`description`、`detail`、`scenario` | 非空字符串；前三项默认来自根说明，scenario 默认取 `when_to_use`，按需要明确覆盖 |
| `version` | 字符串发布标签，不是 artifact、release ID 或草稿 revision |
| `nameEn`、`descriptionEn`、`detailEn` | 可选英文文案字符串 |
| `coverUrl`、`coverInputUrl`、`casePublicLink` | 可选封面、输入封面及案例地址字符串；使用真实上传或已有地址 |
| `tags`、`scenes`、`categoryIds` | 字符串数组；分类身份来自实际已有分类，不凭名称猜 ID |

已有正式版本的工具，可更新 `name/description/detail` 与封面；场景、英文文案、标签、分类或案例链接须保留拉取时的值，不能借重新构建修改。`version` 仍按发布标签处理。封面字段省略会保留原值，显式空字符串表示清除；不要用空值意外抹掉现有封面。

`meta.json` 在打包时单独读取并从执行 ZIP 移除，发布元数据另行传递。工具执行说明或脚本需要的内容放到实际随包交付的文件中，不能在运行时依赖根 `meta.json`。

## 本地校验

```bash
virse tool validate ./product-shot-tool --json
```

本地校验不需要认证，不执行脚本，不上传、不创建 artifact。成功报告包含 `valid`、`offline`、`files`、`excluded`、包摘要与元数据；不能把它当成真实运行效果证明。

校验可能遇到首个错误即返回；修复后须再校验，不能把单次失败报告当成全部问题清单。`excluded` 表示打包排除项，不自动等于 warning；成功报告没有 errors/warnings 字段也不代表逐项提供过诊断。

当前包约束包括 ZIP 不超过 20 MiB、展开内容不超过 100 MiB、最多 2048 个文件、根 `SKILL.md` 和 `interface.json` 各不超过 64 KiB。路径必须可移植，拒绝符号链接、硬链接和特殊文件。CLI 会排除隐藏文件、依赖缓存、构建产物及常见密钥文件，并检查明显的凭据内容；仍需自行确认准备交付的文件完整且不含凭据。

不要因文件被自动排除，就认为它会随包到达运行环境。脚本需要的真实依赖、素材与引用必须可达；本地执行脚本是否属于任务，按用户授权判断，校验不会替你执行。

## 服务端校验

在需要后端权威包报告、准备构建或排查本地与服务端差异时执行：

```bash
virse tool validate ./product-shot-tool --remote --org <org-id> --json
```

命令要求认证与组织，先按同一本地打包约束制作包，再请求服务端检查。结果 `valid=true` 才是服务端接受的校验结果；修复 errors，结合实际用途判断 warnings。该动作不运行工具、不建立产物、不发布；服务端报告正确也不能代替测试真实输出。

按实际 code/path/field/message 转述诊断。`DISCOVERY_HINT_RECOMMENDED` 不能直接解释为缺少 `when_to_use`；先检查原字段，已有字段时保留原文与触发原因未明的边界，不据建议臆断缺失。

包确认后进入[产物测试与发布](artifact-lifecycle.md)，不绕过实际运行。
