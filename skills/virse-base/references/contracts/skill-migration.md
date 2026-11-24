# 旧入口迁移

已有安装更新后，先用安装器列表和实际 Skill 目录核对新旧入口。`skills add` 安装新名称不保证删除旧名称；同一能力保留两份会影响宿主选择，也增加发现层上下文。

使用 `npx skills` 安装时，区分项目级与全局安装：

```sh
npx skills list
npx skills list --global
```

当前对应关系：

| 旧标识 | 当前标识 |
| --- | --- |
| `virse`、`virse-setup` | `virse-base` |
| `aesthetic-creator` | `virse-aesthetic-creator` |
| `smart-tool-creator` | `virse-smart-tool-creator` |
| `brand-center-creator` | `virse-brand-center-creator` |
| `ultra-design` | `virse-ultra-design` |

先确认 `virse-base` 和需要的业务技能来自同一版本、安装在同一范围且可读；`skills add` 不会自动补齐 base 依赖。核实旧目录确属这套 Virse Skill，并与用户敲定清理范围。存在本地修改时先备份到 Skill 发现目录之外，保留需要的定制。新旧名字只是 Skill 标识变化，CLI 可执行命令仍是 `virse`。

在对应项目目录，命令只列实际存在且已确认迁移的旧标识；六个旧入口都存在时可用：

```sh
npx skills remove virse virse-setup aesthetic-creator smart-tool-creator brand-center-creator ultra-design --yes
```

这是项目级操作，针对该项目所有 Agent 的这六个旧入口；全局安装才加 `--global`。共用目录仍被其他 Agent 使用时，限定 `--agent` 的移除可能保留旧目录，因此先协调实际使用范围，不把命令的成功提示当成清理完成。

重读列表与实际目录，确认六个旧入口已消失、当前包仍完整可用；安装器维护 lock 时也核对相应条目。不使用全量卸载，不移除无关 Skill。完成后让宿主重新发现 Skill，必要时新开会话使用当前标识。
