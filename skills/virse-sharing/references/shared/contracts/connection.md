# 连接与操作范围

## 身份与后端

用户指定后端时，在首条远端命令前设置 `VIRSE_BACKEND_URL`；否则保留当前环境的后端选择或使用 CLI 默认值，安装位置按当前执行环境判断。已有信息足够就推进，不固定追问环境。沿用已认证身份，不为环境核对切换账号或修改默认值。出现认证错误、账号不符或需要切换时再查询：

```sh
virse auth status --json
virse config get --json
virse space list --json
```

`VIRSE_BACKEND_URL` 选择后端，未设置时为 `https://api.virse.ai`。profile 不保存后端地址；`--profile` 隔离该后端下的本机凭据和默认组织/画布；`VIRSE_CONFIG_DIR` 改本机存储位置。`VIRSE_API_TOKEN` 优先于本机保存值；改 profile 或 logout 不会屏蔽仍设置的环境 token。

## 安装与首次登录

CLI 尚未安装时，使用用户提供的安装命令，或读取[官方安装说明](https://github.com/Atlas-Of-Imagination/virse-cli#安装与配置)，按当前执行环境与实际脚本输出完成安装并确认 CLI 可用。私有仓库安装方案需要已认证且有仓库读权的 GitHub CLI，并要求安装脚本已进入 main、目标二进制 Release 已发布；仅合入源码不等于已有可下载发行包。安装器只安装 CLI 二进制，Skill 单独安装。未取得可用官方命令或入口时再询问，不猜下载地址或安装参数。已有可用 CLI 和登录态时直接继续任务。

未登录时由 Agent 发起网页登录；对应环境须已部署配套后端授权接口和 Web 授权页。先按当前 `auth login --help` 核对支持的方式；仅展示链接可用 `virse auth login --no-browser`。此模式仍需浏览器访问 CLI 所在机器的本机回调端口；SSH、容器或跨机器授权使用 `--device`，需要时加 `--no-browser`。旧 CLI 不支持时说明需要官方支持的版本，不伪造参数。

授权链接与核对码先输出到 stderr，即使使用 `--json` 也不能只读 stdout。让宿主在命令运行中读取输出，立即向用户展示真实链接和核对码，请其在网页完成登录与确认；保留原等待进程，给用户操作机会，不阻塞到用户看不到链接，也不重复发起登录。宿主无法保留进程时可暂停，用户完成后按 CLI 的恢复方式继续。不要要求用户把 API Key 发进聊天，不展示令牌、PKCE verifier 或领取秘密。

展示链接或用户说“已授权”不代表登录完成。等 CLI 确认领取与保存成功，再用 `virse auth status --json` 核对实际身份，补齐任务需要的空间并继续原任务。超时或中断后，在原 backend/profile 下重跑 `auth login` 恢复原请求；明确过期时，核对没有需要保留的有效授权后按 CLI 提示清理再发起。

自动化或用户明确采用已有 API Key 时，可由秘密来源接入 `auth login --token-stdin`，不把 key 放命令参数、日志或聊天。浏览器授权的 `auth logout` 会先撤销本次服务端 key，再清本地记录；失败时保留记录供重试。手工导入只清本机凭据。`VIRSE_API_TOKEN` 优先于保存值；不能把新保存的授权当作当前生效身份。退出浏览器授权遇 `AUTH_ENV_OVERRIDE` 时，只在用户明确要退出该授权的情况下，对本次命令取消环境覆盖后重试。

## 默认与单次上下文

`org` 和 `canvas` 由当前任务事实取得；下例的 shell 变量都应使用真实查询结果赋值。

```sh
virse config set org "$org" --json
virse config set canvas "$canvas" --json
virse --org "$org" balance --json
```

保存默认值只保存 UUID，不验证可访问性。临时任务优先用相应命令的 `--org`/`--canvas`，不为一次调用改用户默认值。切换空间不会改变远端资源所属空间。余额、预算、许可按返回原值解释；没有上限或未知值不当成零，也不能据余额查询承诺单次任务锁价。

参数不确定时只查正在使用的具体命令 `virse … --help`，不从根命令逐层重复加载 help。没有任意 HTTP 调用或全局 Schema 入口。模型查 `model list/get`，生成操作查 `generation operations`，工具输入查固定 artifact 的 `tool schema`。

## 权限错误

区分认证失败、组织或画布不符、资源无权、模型资格与功能未部署。先检查错误码与当前对象，不自动切换账号，也不把错误当空结果。需要用户补充时说明缺少哪个身份、范围或资格。
