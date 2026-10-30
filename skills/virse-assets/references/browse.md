# 查找与读取

## 定位资源与身份

先用用户给出的路径或 ID，缺少定位才查目录。`resource get /` 返回当前组织根身份；普通目标返回资源项以及后端提供的资产、内容与权限信息。路径只穿过文件夹，不穿过画布；裸 UUID 和 `@UUID` 都按资源项身份解析。同名候选应结合父目录、类型或稳定 ID 选定。

```bash
virse resource get '/项目素材/产品.png' --org "$ORG" --json
virse resource get "$RESOURCE" --org "$ORG" --json
```

已知 asset ID 且有画布访问上下文时才使用资产入口：

```bash
virse asset get "$ASSET" --org "$ORG" --canvas "$CANVAS" --json
```

资源资料支持图片、视频、DOCX、PDF、GLB；发现、上传或下载能力不等于能在 Virse 内编辑、解析全文或渲染所有格式。JSON、TXT、SVG 不作为这些入口已支持的原件格式；任务需要时仅按用户授权转换到可支持格式，保留原件及转换关系，不改扩展名冒充转换。

不要把目录条目的 ID 填入资产接口。需要实际字节时读[文件交付](shared/contracts/delivery.md)，需要分享资格时查询权限，不从团队角色猜资源可用性。标识关系见[对象关系](shared/context/objects.md)。

## 列表与搜索

`ls` 默认列当前目录子项；`search` 默认在所选目录递归搜索。按任务缩小范围，必要时组合类型、名称、本人创建、访问档位和排序条件。

```bash
virse resource ls '/项目素材' --kind image --sort modified-at --order desc --org "$ORG" --json
virse resource search '产品' '/' --view mine --created-by-me --org "$ORG" --json
virse resource ls '/' --view external --org "$ORG" --json
```

视图可选 `all/mine/team-shared/shared/external`；创建者条件只有 `--created-by-me`，没有按任意创建者 UUID 筛选。外部共享入口和原资源不是两个独立文件副本。

每页 1–200 项。收到 `next_cursor` 后保留原目录、筛选、递归和排序条件继续 `--cursor`；核对 `truncated`，不把一页当全集。`snapshot` 是该次目录读取上下文，`sync_cursor` 若返回可用于资源变化接续，不能用目录 `next_cursor` 替代。

## 查看文件夹层级

```bash
virse resource tree '/项目素材' --depth 3 --max-items 2000 --ids --org "$ORG" --json
```

响应过大时优先查目标子目录、限制深度或逐页读取；完整 JSON 保存到任务文件再选取所需字段。工具截断与服务端 `incomplete` 分别核对，未补读不声称全文已读。

画布在资源树中是文件项。检查 `incomplete`、`collapsed` 和 `unplaced`：达到深度或项目上限时，扩大合适范围或改查目标子目录，不宣称已读完整棵树。独立查询的多个子树不构成一份全局一致快照。
