# quality-check CLI 更新规则

> 文档角色：current-reference
> 生命周期状态：current
> 当前真源：docs/INFRA_CLI.md
> 摘要：记录将求解器 Linux CLI 更新到 quality-check 的操作规则与热路径部署

`scripts/update_solver_quality.py` 是指向远端 quality-check（114.66.55.78）的点对点投递脚本，
无参数运行。它只构建并原子投递 Linux ELF，完成 partial → final handoff 后退出；runner
异步建立不可变 bundle，但不会自动创建 job。
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
| 归档目录 | `/var/lib/arknights-infra-quality/v2/discovery-archive/<bundleId>/` |
| 失败目录 | `/var/lib/arknights-infra-quality/v2/discovery-processing/`（保留供重试） |
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

使用唯一入口（脚本自行执行一次 release 构建，不要预先重复构建）：

```bash
python3 scripts/update_solver_quality.py
```

脚本先通过 scp 写入带制品 SHA 的临时文件，再在远端锁保护下检查目标不存在并原子 rename，
避免 runner 领取不完整的文件或并发投递互相覆盖。
handoff 完成后 runner 可能立即取走 final 文件，因此脚本不以 incoming 中持续存在 `infra-cli`
作为成功条件。脚本不会等待 bundle 或 job，也不会调用创建 job 的 API：

```bash
scp dist/arknights-infra-linux-x86_64/infra-cli \
  root@114.66.55.78:/var/lib/arknights-infra-quality/solver-incoming/infra-cli.<sha256>.partial
```

上传完成后，在远端执行权限设置和原子改名：

```bash
ssh root@114.66.55.78 '
  chown arkinfra:arkinfra /var/lib/arknights-infra-quality/solver-incoming/infra-cli.<sha256>.partial &&
  chmod 750 /var/lib/arknights-infra-quality/solver-incoming/infra-cli.<sha256>.partial &&
  test ! -e /var/lib/arknights-infra-quality/solver-incoming/infra-cli &&
  mv /var/lib/arknights-infra-quality/solver-incoming/infra-cli.<sha256>.partial \
      /var/lib/arknights-infra-quality/solver-incoming/infra-cli
'
```

目标文件已存在、远端锁持续 30 秒，或临时文件不存在时，脚本失败并保留现场；不得用覆盖或删除旧文件的方式重试。

正式文件名出现后，quality runner 在 2 秒轮询周期内领取它。不要把新的 CLI 直接
复制到已有 `bundles/<bundleId>/`，也不要修改已有 bundle 的 `infra-cli` 或 `data/`。

## 5. runner 规则

`arknights-infra-quality-next-runner.service` 会：

1. 领取 `solver-incoming` 中的一个完整可执行文件。
2. 计算 CLI SHA-256，启动临时 `serve` 读取 solver 自报的 `git_commit` / `built_at`。
3. 按 solver commit 建立 bundle；没有有效 commit 时使用 CLI SHA-256。
4. **未配置 `QUALITY_AUTO_BUNDLE_DATA_DIR` / `ARKNIGHTS_INFRA_DATA_DIR` 时，
   `embeddedData=true`，跳过 `bake all`，直接使用 CLI 内置 data（热路径）。**
5. 将 CLI 复制到新的不可变 bundle，记录 executable hash；`dataFiles` 为空、
   `bakedManifestSha256` 为 null。
6. 安装成功后将原文件归档到 `v2/discovery-archive/<bundleId>/`。
7. **不会自动创建 job，也不会读取部署机的 `matrix.json`。**

自动发现失败时，文件保留在 `v2/discovery-processing/`，并由 discovery event 记录失败阶段。

需要计算时，另行手动调用现有 Quality API；该动作不属于 ELF 投递脚本。

## 6. 监控与验收

```bash
ssh root@114.66.55.78 'journalctl -u arknights-infra-quality-next-runner.service -f'
```

成功时应看到：

```text
ELF atomically handed to the runner; no job created
```

查看质量服务公开状态：

```bash
curl -sS https://instance-pi2ohhfj.tail2dca9.ts.net:10000/api/quality/status
```

脚本退出只证明 ELF handoff 完成；异步验收至少确认：

- 公开 status API 的 `bundles` 出现新的 `bundleId`、`gitCommit` 和
  `executableSha256`。该摘要当前不提供 `embeddedData`，字段缺失不是部署失败。
- 对应 bundle definition 的 `fingerprint.dataMode` 为 `embedded`、`dataFiles` 为空，
  这是热路径证据；不要从 status 摘要重新推断。
- discovery event 出现 `bundle_published` 和 `archived`，且 `error=null`。
  自动发现不创建 job，因此 `jobId=null` 是正常结果。
- 如需计算，再由人工显式创建 job，并在 `/quality` 页面查看结果。
- runner 日志没有 bake、权限或 fingerprint mismatch 错误。
- 新旧 bundle 均保留，旧 job 的结果仍引用旧 `bundleId`。
- **失败 case 集合与旧 bundle 对比**：新版本不应引入新的失败；同一批 case
  失败且错误消息相同视为无回归。

## 7. 失败处理

| 现象 | 处理 |
|---|---|
| 文件长期留在 `solver-incoming` | 检查 runner 状态、文件大小、owner 和 `0750` 权限 |
| discovery processing 长期未归档或 event 带 error | 查看 discovery event 与 runner 日志；优先检查 Linux 架构、ELF 有效性和 `serve` probe |
| job 全部 `infra_interrupted` | 检查 bundle 内 `infra-cli` 是否为 `0750` 可执行文件 |
| preflight 报 executable/data 被修改 | 不要修补旧 bundle；重新投递新 CLI，生成新 bundle |
| `serve` 报 operbox 路径或解析错误 | 使用与 solver 匹配的合法 operbox；`plan.compute` 内联传入 |
| 投递后没有新 job | 正常；投递入口不创建 job，需要时显式调用 Quality API |
| case 报 `solver_failed` | 打开 job case 看 `errorCode` / `errorMessage`；已知失败集合（如 12F 练度不可能、中枢 required anchors 超容量）需与旧 bundle 比对确认非回归 |

修复后必须使用新的 CLI 文件再次投递。禁止通过修改现有 bundle 文件、删除
manifest 或手工改 job 的 `bundleId` 绕过 preflight。

## 8. 交付记录

每次更新至少记录：

- solver Git commit（`bundle.json` 的 `gitCommit`）和 `builtAt`；
- `infra-cli` SHA-256（`executableSha256`）；
- bundle definition 的 `fingerprint.dataMode` 是否为 `embedded`（热路径）；
- quality-check 新建的 `bundleId`；若另行创建 job，再记录 job ID；
- 显式 job 的 `/api/quality/status` 验收结果（completedCount / failedCount / caseStatuses）；
- 如运行显式 job，记录失败 case 与旧 bundle 的对比结论（是否引入回归）；
- 未解决的失败 case 或数据兼容风险。

该流程只更新 quality-check 的独立 bundle，不会替换主站或开发环境的共享 CLI。
主站与开发环境的求解器更新见
[arknights-solver-deploy skill](../.agents/skills/arknights-solver-deploy/SKILL.md)，
唯一入口为 `scripts/update_solver_production.py`（默认生产，`--environment development`
用于开发环境）。
