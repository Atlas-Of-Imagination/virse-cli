---
name: virse-sharing
description: 查看和调整 Virse 资源访问权限、继承、分享链接与邀请认领；适用于“谁能看”“给他使用”“收回访问”。工具团队可用范围由 virse-tools 管理。
---

# Virse 资源访问与分享

依赖 `virse-base`。首次使用本技能时，从宿主技能列表定位并读取 base 的 `SKILL.md`；已读且适用的内容沿用。下文 `virse-base/references/...` 均相对于该技能的实际目录，按任务读取，不假定安装在全局目录。缺少 base 时先补齐同来源、同版本、同安装范围的 base，再执行依赖它的 Virse 操作。

用户指定后端时，在首条远端命令前设置 `VIRSE_BACKEND_URL`。

## 先确认对象、受众和结果

资源项身份用于分享，不拿 asset ID、画布 Space ID 或团队角色代替。沿用当前已明确身份与组织；确需核对时读连接（`virse-base/references/contracts/connection.md`）或对象身份（`virse-base/references/context/objects.md`）。

用户只问谁能访问时，只读：

```sh
virse --org "$org" resource permissions get "$resource" --json
```

同时解释本人有效访问、直接规则与继承来源。能看规则不代表能管理授权；撤一条规则也不保证彻底失权。设置受众、档位、预览、应用、撤回及清除，直接读[有效权限与直接授权](references/access.md)。目标明确就按已授权范围推进；用户只要求预览时停在预览结果。

## 分享或接受邀请

取得链接不增加任何权限；链接访问要求登录，不代表匿名公开。用户要求“发一个能用的链接”时，先核对或按其授权调整受众资格，再取得地址。接收方解析与认领也不同：只查看目标用 resolve，明确接受到某空间才 claim。

具体短路径见[链接与认领](references/links.md)。不要猜前端域名，也不要为分享制作副本或移动原件。

## 完成标准

报告实际资源、目标受众、应用档位或认领结果。撤权核对剩余来源，改权核对写后设置。`applied:true` 加刷新失败仍是已写入；补读规则，不能重复变更。网络结果未知按执行结果与恢复（`virse-base/references/contracts/execution.md`）先对账。
