# quality-check CLI 更新规则

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/INFRA_CLI.md
> 摘要：记录将求解器 Linux CLI 安全更新到远端 quality-check 的操作规则

本文只描述求解器仓库产物如何进入远端 quality-check。quality-check 会为每个 solver 建立不可变 bundle；不要直接覆盖已有 bundle、运行中的 CLI 或共享 data 目录。

## 1. 更新前提

- 目标服务：`114.66.55.78` 上的独立 quality-check。
- 运行用户：`arkinfra`。
- CLI 必须是 Linux x86-64 可执行文件。
- CLI 必须和配套完整 `data/` 树一起验证。至少需要：
  `operator_instances.json`、`skill_table.json`、`standalone_roster.json`、
  `trade_shortcuts.json`、`trade_segments.json`、`base_systems.json`、
  `training_advice_knowledge.json`。
- 标准回归输入是 `data/fixtures/243/layout.json` 与
  `data/fixtures/243/operbox_full_e2.json`。operbox 必须是当前 solver 可解析的合法练度。
- 不要在 dirty worktree 生成正式 release。release 脚本会拒绝 dirty worktree，避免把未提交代码误送到质量服务。

## 2. 构建并验证 Linux CLI

在本仓库根目录执行：

```bash
git status --short --branch
bash scripts/codex/run_evidence.sh \
  --task quality-cli-build-<commit> \
  --category build \
  --stem release-build \
  --inputs 'commit=<commit>;target=Linux x86-64' \
  -- cargo build --release -p infra-cli

bash scripts/codex/run_evidence.sh \
  --task quality-cli-contract-<commit> \
  --category full-suite \
  --stem workspace-release \
  --inputs 'commit=<commit>;target=Linux x86-64' \
  -- cargo test --release --workspace

bash scripts/codex/run_evidence.sh \
  --task quality-cli-bake-<commit> \
  --category cli \
  --stem bake-and-verify \
  --inputs 'commit=<commit>;data=data;fixture=data/fixtures/243' \
  -- bash scripts/bake_and_verify.sh
```

其中 `<commit>` 使用待发布提交的短 SHA。若只是运行已通过完整验证的 release 构建，也必须保留对应 evidence 目录。

生成正式 Linux release：

```bash
bash scripts/build_release_linux.sh
```

默认产物为：

```text
dist/arknights-infra-linux-x86_64/
├── infra-cli
├── data/
├── fixtures/
└── contracts/
```

构建脚本会把 `SOLVER_GIT_COMMIT` 和 UTC `SOLVER_BUILT_AT` 写入 release 的 `VERSION.txt`，并复制完整 data。计算制品指纹：

```bash
sha256sum dist/arknights-infra-linux-x86_64/infra-cli
```

不要只上传 `target/release/infra-cli` 而遗漏配套 data。quality bundle 会复制并冻结 data，之后会运行 `infra-cli bake all`；data 与 solver 不匹配会导致安装或 preflight 失败。

## 3. 上传到远端

先通过临时文件名上传，避免 runner 领取不完整的文件：

```bash
scp dist/arknights-infra-linux-x86_64/infra-cli \
  root@114.66.55.78:/var/lib/arknights-infra-quality/solver-incoming/infra-cli.partial
```

上传完成后，在远端执行权限设置和原子改名：

```bash
ssh root@114.66.55.78 '
  chown arkinfra:arkinfra /var/lib/arknights-infra-quality/solver-incoming/infra-cli.partial &&
  chmod 750 /var/lib/arknights-infra-quality/solver-incoming/infra-cli.partial &&
  mv /var/lib/arknights-infra-quality/solver-incoming/infra-cli.partial \
     /var/lib/arknights-infra-quality/solver-incoming/infra-cli
'
```

正式文件名出现后，quality runner 通常会在 2 秒轮询周期内领取它。不要把新的 CLI 直接复制到已有 `bundles/<bundleId>/`，也不要修改已有 bundle 的 `infra-cli` 或 `data/`。

## 4. runner 的自动处理规则

`arknights-infra-quality-runner.service` 会：

1. 领取 `solver-incoming` 中的一个完整可执行文件。
2. 计算 CLI SHA-256，并启动临时 `serve` 读取 solver 自报的 `git_commit` / `built_at`。
3. 按 solver commit 建立 bundle；没有有效 commit 时使用 CLI SHA-256。
4. 默认复用当前最新 bundle 的 data 源；首次安装且没有历史 bundle 时，必须配置 `QUALITY_AUTO_BUNDLE_DATA_DIR` 或 `ARKNIGHTS_INFRA_DATA_DIR`。
5. 将 CLI 复制到新的不可变 bundle，执行 `infra-cli bake all`，记录 executable/data/baked manifest hash。
6. 安装成功后将原文件归档到：
   `/var/lib/arknights-infra-quality/solver-archive/<bundleId>/`。
7. 自动创建一份固定质量矩阵 job。已有非终态 job 时不会抢占；新文件会留在 incoming 等待。

自动发现失败时，文件会移动到：

```text
/var/lib/arknights-infra-quality/solver-failed/
```

## 5. 监控与验收

查看 runner 日志：

```bash
ssh root@114.66.55.78 \
  'journalctl -u arknights-infra-quality-runner.service -f'
```

成功时应看到类似：

```text
自动发现 solver：bundle=solver-... job=...
```

查看质量服务公开状态：

```bash
curl -sS \
  https://instance-pi2ohhfj.tail2dca9.ts.net:10000/api/quality/status
```

验收至少确认：

- `bundles` 出现新的 `bundleId`、`gitCommit` 和 `executableSha256`。
- 新 bundle 的质量 job 已创建并进入 `queued` / `running`。
- `/quality` 页面可打开该 job，并能查看 case 结果。
- runner 日志没有 `bake`、data manifest、权限或 fingerprint mismatch 错误。
- 新旧 bundle 均保留，旧 job 的结果仍引用旧 `bundleId`。

## 6. 失败处理

常见故障：

| 现象 | 处理 |
|---|---|
| 文件长期留在 `solver-incoming` | 检查 runner 状态、文件大小、owner 和 `0750` 权限 |
| 文件进入 `solver-failed` | 查看 runner 日志；优先检查 Linux 架构、data 完整性和 `bake all` |
| job 全部 `infra_interrupted` | 检查 bundle 内 `infra-cli` 是否为 `0750` 可执行文件 |
| preflight 报 executable/data 被修改 | 不要修补旧 bundle；重新投递新 CLI，生成新 bundle |
| `serve` 报 operbox 路径或解析错误 | 使用与 solver 匹配的完整 data 和合法 `data/fixtures/243/operbox_full_e2.json` 重建 release |
| 新 job 未出现 | 当前已有非终态 job；等待完成或按 quality-check 运维规则取消后再投递 |

修复后必须使用新的 CLI 文件再次投递。禁止通过修改现有 bundle 文件、删除 manifest 或手工改 job 的 `bundleId` 绕过 preflight。

## 7. 交付记录

每次更新至少记录：

- solver Git commit 和 `VERSION.txt` 中的 build time；
- `infra-cli` SHA-256；
- 配套 data 来源及其验证 evidence 路径；
- quality-check 新建的 `bundleId` 和 job ID；
- `/api/quality/status` 验收结果；
- 未解决的失败 case 或数据兼容风险。

该流程只更新 quality-check 的独立 bundle，不会替换生产 beta 的 CLI。生产 beta 的 CLI 更新仍遵循前端仓库的 `docs/UPDATE_SOLVER.md`。
