# 有效权限与直接授权

## 查询有效访问

```sh
virse --org "$org" resource permissions get "$resource" --json
```

同时看 `my_access`、`can_manage`、规则来源、可修改上限与可移除状态。创建者、直接成员、组、资源所属团队、链接和祖先继承可能共同提供访问；撤掉其中一项不代表完全失权。团队角色不替代资源权限，读取规则也不意味着有权管理。资源详情与权限接口的创建者标记或有效访问冲突时，保留两方原值与来源，按权限接口的实际操作许可继续核对，不把冲突静默改写成一致结论。

当前 JSON 中的数值档位对应 `0=none`、`1=view`、`2=use`、`3=edit`、`4=manage`。`my_access` 是调用者自己的有效档位，不是正在撤权的目标成员；目标成员还需结合其规则与继承来源判断。

修改仅针对本层规则。继承与创建者行不能在子资源直接修改；想撤销继承，先找真实来源，不能擅自扩大到父目录。移动资源可能改变继承，复制会创建独立对象；整理目录时一起核对真实访问变化。

## 受众与档位

| 受众参数 | 可设档位 | 识别范围 |
| --- | --- | --- |
| `--member` | view/use/edit/manage | 用户 UUID 或 `用户UUID@组织UUID`，裸用户 UUID 使用本次 org |
| `--email` | view/use/edit/manage | 外部邮箱待认领关系；不自动加入团队 |
| `--group` | view/use/edit | 真实成员组 UUID；撤回用规则 ID remove，不能设 none 或 manage |
| `--org-members` | none/view/use/edit | 资源所属组织的全体成员，个人空间无该受众 |
| `--link` | none/view/use/edit | 已登录且持有链接的人，不能解释为匿名公开 |

CLI 收到档位还会按受众校验，并由服务端限制实际可授予上限。`view`、`use`、`edit`、`manage` 使用服务端定义；不要据某一档位推断所有媒体原件、工具源码或作者操作都开放。

## 预览与应用

需要了解影响或用户要求先看时，先做相应 dry-run。真实 set/remove 调用本身也会请求影响预览；预览失败阻止本次写入。单独预览与以后执行不构成原子事务。

```sh
virse --org "$org" resource permissions set "$resource" --member "$member" --role use --dry-run --json
virse --org "$org" resource permissions set "$resource" --member "$member" --role use --json
```

只对用户指定的受众和档位应用。写后核对返回设置或重新读取；结果不确定先对账，不盲目重发。`applied:true` 即便 `settings:null` 且有 `refresh_error`，也代表变更已经应用，只需补读规则。

## 撤回与清除

针对已读取、可移除的本层 rule ID：

```sh
virse --org "$org" resource permissions remove "$resource" "$rule" --dry-run --json
virse --org "$org" resource permissions remove "$resource" "$rule" --json
```

固定团队/链接受众用对应 `set --role none`。关闭本层链接不清除继承链接或具名授权。`permissions clear --dry-run` 只展示拟清除的本层设置，没有合并受影响人数；真实 clear 清掉本层规则与本层链接档位，保留创建者和继承，也不删除原件。

撤权后核对剩余来源。如果用户目标是“这个人不能再访问”，给出仍然授予访问的准确来源；不能只报告已删一行就完成。

当前 CLI 没有任意目标成员的完整有效访问解析或组成员展开入口。规则明确保留的访问可以指出；缺少该成员的其他身份与组归属依据时，不能仅凭规则列表或本人的 `my_access` 证明他已彻底失权，应说明已经撤回的范围与仍无法确认的部分。
