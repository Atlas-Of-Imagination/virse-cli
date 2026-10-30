# 二进制发布

`.github/workflows/release.yml` 参考 [kitkit-cli](https://github.com/stello-agent/kitkit-cli/blob/main/.github/workflows/release.yml)，使用 `taiki-e/setup-cross-toolchain-action`、`taiki-e/upload-rust-binary-action` 和 `taiki-e/create-gh-release-action` 构建及发布实际二进制 `virse`。

## 平台与产物

| 平台 | Rust target | 格式 |
| --- | --- | --- |
| Linux x64，musl | `x86_64-unknown-linux-musl` | `.tar.gz` |
| Linux ARM64，musl | `aarch64-unknown-linux-musl` | `.tar.gz` |
| Linux x64，glibc | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc` | `.zip` |
| Windows ARM64 | `aarch64-pc-windows-msvc` | `.zip` |

文件名为 `virse-<tag>-<target>.<格式>`，校验文件为 `virse-<tag>-<target>.sha256`。**压缩包内只包含根目录下的 `virse` 或 `virse.exe`**，没有 README、docs、skills 或其他内容；安装脚本 `install.sh`、`install.ps1` 作为独立 Release 附件上传。Linux GNU 版本在 Ubuntu 22.04 上构建；Linux 安装器默认选择 musl，避免依赖主机 glibc 版本，不保证 GNU 版本能在更旧的 glibc 上运行。

## 安装器

参考 kitkit-cli 提供两种安装入口，默认解析最新稳定版，也接受 `1.2.3` 或 `v1.2.3` 等明确版本。脚本下载并校验对应归档的 SHA-256，校验通过后才替换旧二进制。只安装 bin，临时下载文件退出时清理；不会自动申请 sudo。检测到已认证的 GitHub CLI 时通过 `gh release` 查询、下载，支持当前私有仓库；否则使用 HTTPS 匿名下载公开 Release。

```bash
# macOS / Linux：下载脚本后可指定版本、目录。
sh install.sh --version v0.1.0 --dir "$HOME/.local/bin"
# 可选：--target x86_64-unknown-linux-gnu、--repo owner/repo。
```

```powershell
# Windows：默认自动配置用户 PATH，可选择不修改。
.\install.ps1 -Version v0.1.0 -InstallDir "$env:LOCALAPPDATA\Programs\virse\bin"
.\install.ps1 -NoPathUpdate
# 可选：-Target aarch64-pc-windows-msvc、-Repository owner/repo。
```

两种脚本都读取 `VIRSE_VERSION`、`VIRSE_INSTALL_DIR`、`VIRSE_TARGET`、`VIRSE_REPO`；命令参数优先。Unix 使用 PATH 中已存在且可写的 `~/.local/bin`、`~/bin`、`/opt/homebrew/bin` 或 `/usr/local/bin`，否则创建 `~/.local/bin`。不直接修改 Shell 配置文件，目录不在 PATH 时打印配置命令。Windows 默认使用 `%LOCALAPPDATA%\Programs\virse\bin`，用户 PATH 和当前进程 PATH 分别检查并补入；已有终端可能需要重开。

也可手动下载压缩包与对应校验文件，校验后解压，将单个二进制放入 PATH 中的目录。当前私有仓库需先 `gh auth login`，或为 GitHub CLI 配置有仓库读取权限的 `GH_TOKEN`。安装器不保存 Token。通过 GitHub CLI 获取脚本的命令见 README。

如果后续发行仓库公开，可直接使用 HTTPS 入口：

```bash
curl -fsSL https://github.com/Atlas-Of-Imagination/virse-cli/releases/latest/download/install.sh | sh
```

```powershell
irm https://github.com/Atlas-Of-Imagination/virse-cli/releases/latest/download/install.ps1 | iex
```

## 验证与发布

- 向 main 提交 PR 时，7 个目标均执行 `--locked` release 构建、打包和 SHA-256 生成，检查归档只含 bin，并离线测试当前宿主平台的安装脚本（模拟下载，不访问线上或修改真实用户 PATH），产物保存在 Actions artifacts 中 7 天；原生目标额外运行 `--version` 和 `--help`。交叉编译目标只验证构建与打包，不宣称已在对应设备运行。
- 手动执行 workflow 也只构建及保存 artifacts，不发布 Release。非 tag 构建使用 commit SHA 命名产物。
- 推送版本 tag 时，先检查 tag 与根 `Cargo.toml` 的 package version 完全一致。所有平台构建成功、汇总校验通过后才创建草稿 Release，上传全部产物，再公开发布。构建失败不会公开缺少平台产物的新版本；上传失败保留草稿供排查和重跑。

发版前更新根 `Cargo.toml` 的 CLI 版本及 `Cargo.lock`，提交 PR 并等待构建通过；SDK 版本按 SDK 自身变化管理。合入 main 后，在该提交上打 tag：

```bash
git tag v0.1.0
git push origin v0.1.0
```

将示例版本替换为实际版本。`v0.2.0-rc.1` 等预发布 tag 同样要求 Cargo.toml 版本匹配，并由 Release Action 标记为 prerelease。不要修改已公开版本的 tag；修复通过新版本发布。

发布使用 GitHub 自动提供的 `GITHUB_TOKEN`；仅 publish job 需要 `contents: write`，无需额外 PAT。仓库目前没有 CHANGELOG 或 LICENSE 文件，因此不引用不存在的文件，也不替项目声明许可证。本流程不提供自动更新器或平台代码签名。
