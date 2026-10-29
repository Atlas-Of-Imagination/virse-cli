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

文件名为 `virse-<tag>-<target>.<格式>`，每个压缩包附带对应 `.sha256` 文件，包内包含二进制、README 和 docs。Linux GNU 版本在 Ubuntu 22.04 上构建；其他 Linux 发行版优先选择 musl 版本，不保证 GNU 版本能在更旧的 glibc 上运行。

## 验证与发布

- 向 main 提交 PR 时，7 个目标均执行 `--locked` release 构建、打包和 SHA-256 生成，产物保存在 Actions artifacts 中 7 天；原生目标额外运行 `--version` 和 `--help`。交叉编译目标只验证构建与打包，不宣称已在对应设备运行。
- 手动执行 workflow 也只构建及保存 artifacts，不发布 Release。非 tag 构建使用 commit SHA 命名产物。
- 推送版本 tag 时，先检查 tag 与根 `Cargo.toml` 的 package version 完全一致。所有平台构建成功、汇总校验通过后才创建草稿 Release，上传全部产物，再公开发布。构建失败不会公开缺少平台产物的新版本；上传失败保留草稿供排查和重跑。

发版前更新根 `Cargo.toml` 的 CLI 版本及 `Cargo.lock`，提交 PR 并等待构建通过；SDK 版本按 SDK 自身变化管理。合入 main 后，在该提交上打 tag：

```bash
git tag v0.1.0
git push origin v0.1.0
```

将示例版本替换为实际版本。`v0.2.0-rc.1` 等预发布 tag 同样要求 Cargo.toml 版本匹配，并由 Release Action 标记为 prerelease。不要修改已公开版本的 tag；修复通过新版本发布。

发布使用 GitHub 自动提供的 `GITHUB_TOKEN`；仅 publish job 需要 `contents: write`，无需额外 PAT。仓库目前没有 CHANGELOG 或 LICENSE 文件，因此不引用不存在的文件，也不替项目声明许可证。本流程提供二进制压缩包，不生成一行安装脚本、自动更新器或平台代码签名。
