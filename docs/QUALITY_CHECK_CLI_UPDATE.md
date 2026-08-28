# quality-check CLI 更新规则

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/INFRA_CLI.md
> 摘要：记录将求解器 Linux CLI 更新到 quality-check 的操作规则与热路径部署

`scripts/update_solver_quality.py` 是指向远端 quality-check（114.66.55.78）的点对点投递脚本，
无参数运行。quality-check 为每个 solver 建立不可变 bundle；
不要直接覆盖已有 bundle、运行中的 CLI 或共享 data 目录。

## 1. 更新前提

> 本节记录固定远端 quality-check 的投递前提。

- 目标服务：`114.66.55.78` 上的 quality-check（`arknights-infra-quality-next`）。
- 运行用户：`arkinfra`。
- CLI 必须是 Linux x86-64 可执行文件。
- **bake 已暂时废弃**（等待重新设计），quality 当前全部走**热路径**：
  只要不配置 `QUALITY_AUTO_BUNDLE_DATA_DIR` 或 `ARKNIGHTS_INFRA_DATA_DIR`，
  runner 就按 `embeddedData=true` 安装 bundle，**不执行 `infra-cli bake all`**，
  CLI 使用内置 data 直接跑 case。因此上传单个 Linux ELF 即可，不需要配套外部 data。
- 服务器上旧 `arknights-infra-quality.service` / `arknights-infra-quality-runner.service`
  已停用（failed）；当前生效的是 `arknights-infra-quality-next.service` 与
  `arknights-infra-quality-next-runner.service`。

## 2. 环境事实（真源：systemd 运行时）

| 项 | 值 |
|---|---|
| 存储目录 | `/var/lib/arknights-infra-quality`（`QUALITY_STORAGE_DIR`） |
| 投递目录 | `/var/lib/arknights-infra-quality/solver-incoming` |
| 归档目录 | `/var/lib/arknights-infra-quality/solver-archive/<bundleId>/` |
| 失败目录 | `/var/lib/arknights-infra-quality/solver-failed/` |
| Runner | `arknights-infra-quality-next-runner.service`，轮询 2s |
| Next 服务 | `arknights-infra-quality-next.service`，`127.0.0.1:4385` |
| nginx 回环 | `127.0.0.1:4376` |
| 公网入口 | Tailscale Funnel TLS `https://instance-pi2ohhfj.tail2dca9.ts.net:10000` |
| 状态 API | `https://instance-pi2ohhfj.tail2dca9.ts.net:10000/api/quality/status` |

## 3. 构建 Linux CLI

```bash
git status --short --branch
cargo build --release -p infra-cli
```

正式发布从 `scripts/build_release_linux.sh` 产物取 `infra-cli`；该脚本要求 clean
worktree。**当前工作树 dirty 时**可手动构建 release bundle（跳过 clean 检查），
但必须保证 `target/release/infra-cli` 是本次要发布的完整源码构建，并记录
`SOLVER_GIT_COMMIT`（= 当前 HEAD）与 `SOLVER_BUILT_AT`（UTC），二者会编译进
二进制并被 runner 读为 bundle 的 `gitCommit` / `builtAt`。

> 注意：`SOLVER_GIT_COMMIT` 是构建时 HEAD，不包含未提交改动。dirty 构建的
> 二进制含未提交代码，但 bundle 的 gitCommit 仍显示 HEAD；需要以
> executable SHA-256 为准区分制品。若要 clean 语义，请先提交。

## 4. 上传并原子改名

使用固定远端脚本（等待 bundle 和 job 完成）：

```bash
python3 scripts/update_solver_quality.py
```

脚本先通过 scp 写入 `infra-cli.partial`，再在远端原子 rename，避免 runner 领取不完整的文件，
随后轮询远端 status API，直到新 bundle 与质量 job 达到终态。正常的代表性矩阵通常在数十秒内完成；
脚本最多等待 1800 秒，并在终态输出 case 计数：

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

正式文件名出现后，quality runner 在 2 秒轮询周期内领取它。不要把新的 CLI 直接
复制到已有 `bundles/<bundleId>/`，也不要修改已有 bundle 的 `infra-cli` 或 `data/`。

## 5. runner 的自动处理规则

`arknights-infra-quality-next-runner.service` 会：

1. 领取 `solver-incoming` 中的一个完整可执行文件。
2. 计算 CLI SHA-256，启动临时 `serve` 读取 solver 自报的 `git_commit` / `built_at`。
3. 按 solver commit 建立 bundle；没有有效 commit 时使用 CLI SHA-256。
4. **未配置 `QUALITY_AUTO_BUNDLE_DATA_DIR` / `ARKNIGHTS_INFRA_DATA_DIR` 时，
   `embeddedData=true`，跳过 `bake all`，直接使用 CLI 内置 data（热路径）。**
5. 将 CLI 复制到新的不可变 bundle，记录 executable hash；`dataFiles` 为空、
   `bakedManifestSha256` 为 null。
6. 安装成功后将原文件归档到 `solver-archive/<bundleId>/`。
7. 自动创建一份固定质量矩阵 job。已有非终态 job 时不会抢占；新文件会留在 incoming 等待。

自动发现失败时，文件移动到 `solver-failed/`。

## 6. 监控与验收

```bash
ssh root@114.66.55.78 'journalctl -u arknights-infra-quality-next-runner.service -f'
```

成功时应看到：

```text
自动发现 solver：bundle=solver-<commit>... job=<jobId>
```

查看质量服务公开状态：

```bash
curl -sS https://instance-pi2ohhfj.tail2dca9.ts.net:10000/api/quality/status
```

验收至少确认：

- `bundles` 出现新的 `bundleId`、`gitCommit` 和 `executableSha256`，且
  `embeddedData: true`（确认走热路径）。
- 新 bundle 的质量 job 已创建并进入 `running`，最终 `completed`。
- `/quality` 页面可打开该 job，并能查看 case 结果。
- runner 日志没有 bake、权限或 fingerprint mismatch 错误。
- 新旧 bundle 均保留，旧 job 的结果仍引用旧 `bundleId`。
- **失败 case 集合与旧 bundle 对比**：新版本不应引入新的失败；同一批 case
  失败且错误消息相同视为无回归。

## 7. 失败处理

| 现象 | 处理 |
|---|---|
| 文件长期留在 `solver-incoming` | 检查 runner 状态、文件大小、owner 和 `0750` 权限 |
| 文件进入 `solver-failed` | 查看 runner 日志；优先检查 Linux 架构、ELF 有效性和 `serve` probe |
| job 全部 `infra_interrupted` | 检查 bundle 内 `infra-cli` 是否为 `0750` 可执行文件 |
| preflight 报 executable/data 被修改 | 不要修补旧 bundle；重新投递新 CLI，生成新 bundle |
| `serve` 报 operbox 路径或解析错误 | 使用与 solver 匹配的合法 operbox；`plan.compute` 内联传入 |
| 新 job 未出现 | 当前已有非终态 job；等待完成或按 quality-check 运维规则取消后再投递 |
| case 报 `solver_failed` | 打开 job case 看 `errorCode` / `errorMessage`；已知失败集合（如 12F 练度不可能、中枢 required anchors 超容量）需与旧 bundle 比对确认非回归 |

修复后必须使用新的 CLI 文件再次投递。禁止通过修改现有 bundle 文件、删除
manifest 或手工改 job 的 `bundleId` 绕过 preflight。

## 8. 交付记录

每次更新至少记录：

- solver Git commit（`bundle.json` 的 `gitCommit`）和 `builtAt`；
- `infra-cli` SHA-256（`executableSha256`）；
- `embeddedData` 是否为 true（热路径）；
- quality-check 新建的 `bundleId` 和 job ID；
- `/api/quality/status` 验收结果（completedCount / failedCount / caseStatuses）；
- 失败 case 与旧 bundle 的对比结论（是否引入回归）；
- 未解决的失败 case 或数据兼容风险。

该流程只更新 quality-check 的独立 bundle，不会替换 dev 或生产 beta 的 CLI。
dev 更新见 [更新服务器求解器（dev 环境）](UPDATE_SOLVER.md)；生产 beta 的 CLI
更新仍遵循前端仓库的 `docs/UPDATE_SOLVER.md`。
