# Virse CLI

Rust 命令行客户端，面向交互使用、脚本和 Agent。第一阶段已经实现：API Token 认证、画布读取、组合创建元素与生成任务、WebSocket 观察，以及资产读取和下载。

## 架构

```text
virse-cli（bin: virse）
    参数、环境变量、Profile、凭据文件、恢复记录、输出
                     ↓
virse-api（异步 lib）
    类型化请求与任务、Bearer 认证、HTTP、WebSocket、错误
                     ↓
Virse Backend
```

| 职责 | `virse-api` | `virse-cli` |
| --- | --- | --- |
| 配置 | 显式接收 Backend 地址与 Token | 解析环境变量与 Profile |
| API | 请求、响应、权限错误、资产流式下载 | 命令和参数转换 |
| 生成 | 提交、查询、观察、取消 | 同 key 重试、等待超时、Ctrl+C、恢复记录 |
| 持久化 | 不读环境、不读写用户文件 | 凭据、默认组织/画布、提交身份 |
| 输出 | 返回数据、错误、任务快照流 | stdout 结果、stderr 进度/错误、退出码 |

仓库根是 `virse-cli` package，`virse-api/` 是可独立依赖的 SDK。仅这两个 crate，不依赖前端代理、MCP 或浏览器登录。

## 安装与配置

```bash
cargo install --path . --locked --jobs 2
virse --help
```

Backend 默认 **`https://api.virse.ai`**，**只能用环境变量覆盖**：

```bash
export VIRSE_BACKEND_URL=https://dev4.virse.ai
```

不支持 `--backend`、`--backend-url` 或 Profile 中的 Backend 地址。变量未设置才使用 production；空值、非法 URL、含账号/密码、查询参数或路径的地址会报错。HTTP 和 WebSocket 使用相同 origin，自动将 HTTPS 转换为 WSS。

| 环境变量 | 用途 |
| --- | --- |
| `VIRSE_BACKEND_URL` | Backend origin；未设置时使用 production |
| `VIRSE_API_TOKEN` | 当前进程 API Token，优先于保存的凭据 |
| `VIRSE_CONFIG_DIR` | 覆盖本地状态目录，便于 CI 和隔离测试 |

未指定状态目录时使用系统用户配置目录，例如 Linux 的 `$XDG_CONFIG_HOME/virse` 或 `~/.config/virse`。`--profile <name>` 选择命名 Profile，默认为 `default`；凭据、组织和画布默认值均按 **Profile + Backend** 隔离。Backend 身份用于隔离记录，不是可保存的地址配置。

```bash
virse auth login                    # 隐藏输入 virse_sk_… Token
# 自动化使用 VIRSE_API_TOKEN，或通过 stdin 导入：
secret-manager-command | virse auth login --token-stdin
virse auth status
virse config set org <org-uuid>
virse config set canvas <canvas-uuid>
virse config get
```

登录通过 `/auth/me` 验证成功后保存 Token。Unix 状态目录权限为 `0700`、文件为 `0600`，使用原子替换和写锁。凭据保存在本地文件，尚未接入系统钥匙串。非交互登录必须使用 `--token-stdin`，不提供明文 Token 参数。

`auth logout` 仅删除当前 Backend/Profile 的本地凭据，不撤销服务器 Token；如果仍设置 `VIRSE_API_TOKEN`，它仍然生效。

## 从画布到生成资产

### 1. 读取画布和引用

```bash
virse canvas list --org <org-uuid> --limit 50 --json
virse canvas list --org <org-uuid> --cursor '<next_cursor>' --json
virse canvas get <canvas-uuid> --json
virse generation operations --canvas <canvas-uuid> --json
```

`canvas list` 使用带组织权限校验的 `/api/v1/resources/query`，只查询 `canvas`，支持游标分页。返回条目的 `id` 是资源目录 ID，**`target_id` 才是后续使用的画布 ID**。组织 ID 来自现有工作空间；首版不提供组织发现命令。

`canvas get` 返回项目和元素，使用元素 `id` 作为引用。操作 Schema 描述业务输入；模型 key 需使用目标环境模型目录中的实际值，CLI 不硬编码默认模型。

### 2. 一次请求放置元素并生成

```bash
virse generation image \
  --canvas <canvas-uuid> \
  --model <model-key> \
  --prompt '保留 $1 的主体，改为白色背景' \
  --reference <source-element-uuid> \
  --x 100 --y 200 --width 512 --height 512 \
  --wait --json
```

这是一个 `POST /v2/canvas/projects/{id}/generations` 请求，包含 `operation/input/outputs` 和 `Idempotency-Key`。默认通过 `outputs[].create_element` 指定占位；后端在同一事务中创建占位、任务、输出绑定和必要的额度预占，再异步生成并绑定结果。CLI 不提前单独创建元素，也不自行补绑结果。

- 默认新元素位置 `(0,0)`，显示尺寸 `1024×1024`，可以覆盖；显示尺寸不是模型输出分辨率。
- 重复 `--reference` 按输入顺序映射 `$1`、`$2`，传递 `canvas_element` 来源，不把引用转换为 URL。Shell 中含 `$1` 的提示词请使用单引号。
- 对已有元素生成使用 `--target-element <uuid>`，与位置和尺寸参数互斥。
- 生图快捷命令生成单张图片；默认立即返回已接受的任务快照，只有 `--wait` 才等待完成。

### 3. 通用提交

```bash
virse generation submit --canvas <canvas-uuid> --file request.json --json
cat request.json | virse generation submit --canvas <canvas-uuid> --file - --wait
```

`request.json` 示例（替换实际 model 和元素 ID）：

```json
{
  "operation": "image_generate",
  "input": {
    "model": "<model-key>",
    "user_query": "保留 $1 的主体，修改背景",
    "image_count": 1,
    "references": {
      "$1": {
        "source": {
          "kind": "canvas_element",
          "element_id": "00000000-0000-0000-0000-000000000001"
        }
      }
    }
  },
  "outputs": [{
    "output_index": 0,
    "create_element": {
      "position": { "x": 100, "y": 200 },
      "size": { "width": 512, "height": 512 }
    }
  }]
}
```

可以通过同一入口调用后端已支持的多图、视频、文字和 3D 操作，参数以 `generation operations` 返回的 Schema 为准。首版只对图片提供快捷参数；没有新增 3D 截图渲染能力。现有 API 仍要求明确输出绑定，通用提交不是独立于画布的资产生成接口。

### 4. 观察和恢复

```bash
virse generation get <task-uuid> --canvas <canvas-uuid> --json
virse generation watch <task-uuid> --canvas <canvas-uuid> --timeout 900 --json
virse generation cancel <task-uuid> --canvas <canvas-uuid>
```

观察通过现有 Canvas WebSocket 的任务订阅进行，接收当前快照与后续更新，按 revision 去重。初次观察及断线恢复时有限查询任务，重连后重新订阅，不周期轮询。

- 等待默认 600 秒，可用 `--timeout` 修改。超时或 Ctrl+C 仅停止本地等待，不取消服务端生成。
- `needs_attention` 结束本次等待并提示待核对，不伪装成失败，不自动重试供应商。
- 取消按后端真实能力执行；运行中任务不支持取消时保留服务器错误。
- 提交会在 stderr 输出幂等 key。首次请求加最多三次自动重试始终使用相同 key 和请求体。
- 本地记录仅保存 Backend、账号、画布、key、请求摘要与受理后的 task ID，不保存提示词或 Token 副本。恢复记录不自动重放。
- 请求受理结果不确定时，使用**同一请求**和 `--idempotency-key <原key>` 再次提交确认；不要换新 key。相同 key 搭配不同请求会被拒绝。新的显式提交默认产生新 key。

### 5. 获取资产

```bash
virse asset get <asset-uuid> --org <org-uuid> --canvas <canvas-uuid> --json
virse asset download <asset-uuid> --org <org-uuid> --canvas <canvas-uuid> --output result.png
```

资产 ID 来自任务的 `outputs[].asset_id`。文本等操作以任务 `result` 和画布内容为准，不能假设每个输出都有可下载资产。

下载先读取受权限保护的下载信息，再流式写入临时文件，成功后发布目标文件；已有文件不覆盖，跨源下载/跳转不携带 Backend Token。下载失败不会重新执行生成。

下载保留服务端原始字节，指定文件扩展名不会转换图片格式。

## 输出与退出码

默认输出格式化 JSON，`--json` 输出单行 JSON；`generation watch --json` 每行一个 `task_snapshot` 事件。stdout 只输出结果，进度和错误写 stderr；JSON 模式下错误为 `{"error":{...}}`，包含业务码、HTTP 状态和可用的请求 ID；等待错误附带 `task_id`，受理结果不确定时附带 `idempotency_key`。

| 退出码 | 含义 |
| --- | --- |
| 0 | 命令成功；普通查询成功读取失败任务也属于查询成功 |
| 1 | 其他业务、本地配置或文件错误 |
| 2 | 参数错误 |
| 3 / 4 | 认证 / 权限错误 |
| 5 | 网络、服务或观察连接不可用 |
| 6 | 等待的任务失败、部分成功或取消 |
| 7 | 提交结果不确定或任务待核对 |
| 8 | 等待超时 |
| 130 | Ctrl+C 停止等待 |

## 独立使用 SDK

```rust,no_run
use virse_api::Client;

# async fn example() -> virse_api::Result<()> {
let client = Client::new("https://api.virse.ai", "virse_sk_example")?;
let identity = client.me().await?;
# Ok(())
# }
```

SDK 的 `submit` 是单次调用，幂等 key 由调用方传入；自动重试和跨进程记录属于 CLI。`watch` 返回可取消的异步快照流，丢弃流不取消任务。SDK 不保存文件，下载返回可流式读取的响应。

## 验证与当前边界

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
cargo test --workspace -j 2 -- --test-threads=2
```

本地测试通过模拟 HTTP/WebSocket 验证协议及 CLI 子进程行为，不访问业务数据库或模型供应商。真实联调需显式设置测试环境 `VIRSE_BACKEND_URL` 和测试 Token；不要让测试生成落到默认 production。

2026-10-27 已在 dev5 完成真实 API Token 联调：画布列表/元素读取、引用已有图片组合创建、Seedream 4 生成、任务观察（running → succeeded）、资产详情与下载、同 key 重放和已完成任务恢复均通过。只新增一个任务和一个元素，原引用元素保持不变。联调修复了画布读取传空 `expand` 被后端拒绝的问题，现使用协议要求的 `expand=none`，并补充回归测试。此记录不代表其他模型、视频/3D 或生产环境已验收。

目标 Backend 需要已部署 V2 生成接口、任务迁移及计费接管，并启用 `CANVAS_GENERATION_TASKS_ENABLED`。服务未启用时原样返回错误，不降级到旧入口重复生成。

本阶段不包含更新检测、兼容性协商、强制升级、系统钥匙串、自动更新、独立元素创建、工具与审美命令。未来更新机制仍区分 API 契约版本、SDK 版本和 CLI 发布版本：协议处理归 SDK，提示和发行渠道处理归 CLI；本轮不发送兼容性请求或版本协商头。
