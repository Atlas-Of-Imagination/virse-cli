# 链接与认领

## 取得分享地址

```sh
virse --org "$org" resource share link "$resource" --json
```

默认返回服务端相对地址；需要完整浏览器链接时，给用户明确提供或当前产品环境已确认的 `--web-origin`。不要从 API 后端域名猜前端地址。保留返回的 `resource_id`、`link_url` 与可访问信息；此命令只取地址，不授予权限。

分享给某个人或持链接者之前，用[有效权限](shared/contracts/access.md)核对具体受众。若用户仅要复制现有链接，取得地址即可，不擅自开放访问。

## 解析链接

```sh
virse --org "$receiving_org" resource share resolve "$link" --json
```

`link` 使用真实支持的资源/画布 URL 或 token，当前组织是接收上下文。根据返回目标和访问结果继续；解析不等于认领、复制、移动或加入团队。无权限错误不当成链接不存在，也不自动试其他组织。

## 接受邀请

用户明确接受到已选空间后，二选一：

```sh
virse --org "$receiving_org" resource share claim "$link" --json
virse --org "$receiving_org" resource share claim --resource "$resource_id" --json
```

核对实际接收关系和接收空间。关系写入成功与接收方入口可见分别验证：使用同一接收组织与来源 resource ID 查询资源入口；查不到时保留关系结果和可见性差异，不把认领成功说成已出现，也不切换身份反复认领。认领不会改变原件所属空间，也不会创建独立副本。结果未知时先查原分享关系及接收方资源入口，不重发 claim 碰运气。

本人作为创建者的访问不能证明另一受邀者的档位或实际操作已验证；没有接收者身份下的证据时，只报告已确认的规则和接收关系。
