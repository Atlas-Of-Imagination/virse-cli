# 源码与作者管理

## 拉取本人源码

修改已有工具或参考本人历史正式版本时，使用尚不存在的目录：

```bash
virse tool pull <tool-id> --output ./tool-source --org <org-id> --json
virse tool pull <tool-id> --version <published-version> --output ./historical-source --org <org-id> --json
```

此入口要求工具作者身份，不覆盖已有目录。拉取恢复选定 artifact 的原生包与当时元数据，同时保留当前正式发布基准；历史源码与当前乐观锁基准是两份事实，不能把历史 release 当成当前基准强行覆盖。

保留拉取建立的本地绑定。修改后按[原生包与校验](native-package.md)检查，再按[产物测试与发布](artifact-lifecycle.md)构建确定产物。

## 查看本人正式版本

```bash
virse tool versions <tool-id> --org <org-id> --json
```

返回正式 release、版本标签和对应不可变 artifact；该接口供作者管理。若 checksum 为 null，保留该事实，通过实际 release → artifact 身份及已有 build/validate 包摘要建立证据映射；不能把本地摘要说成 versions 已返回的校验结果。普通使用者选择已发布版本通过 `tool schema --version <published-version>` 解析，不需要作者 versions 权限。

## 放弃已知草稿修订

只有明确要放弃该修订且已取得真实 revision 身份时执行：

```bash
virse tool draft abandon <tool-id> <revision-id> --org <org-id> --json
```

revision 必须是正常编辑器提供的真实编辑草稿修订 ID，不是 artifact、release、设计修订或本地版本标签。当前 CLI 只有 `draft abandon`，没有 draft list/get，`tool get` 也不保证提供编辑修订；拿不到真实身份时停止该动作，不能猜命令或 ID。

动作不改变当前正式发布版本。请求只尝试一次；结果未知时保留回执，使用已有授权的正常编辑器核对草稿，不盲目继续发布。放弃旧草稿不会把本地未应用的修改自动合并到新源码。

发布遇 `CREATOR_SESSION_ACTIVE` 等会话冲突时，先保留正式版本和活动会话，定位冲突并按用户意图处理；不能为绕过冲突强制清除或擅自放弃别人的工作。

## 上传封面素材

```bash
virse tool cover upload ./cover.png --json
```

要求认证，CLI 读取上限为 10 MiB。上传回执只是可用封面素材，不会修改工具正式封面或发布内容。将返回的真实素材信息填入本地 `meta.json` 支持的 `coverUrl` 等相应字段，再重新构建并按要求发布。不要猜返回 URL 或把上传成功说成正式封面已更新。

工具的使用开关、收藏、添加和作者有权管理的团队范围属于已有工具管理；不为这些动作重新编写或发布工具包。
