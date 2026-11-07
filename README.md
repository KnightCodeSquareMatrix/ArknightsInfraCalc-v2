# ArknightsInfraCalc v2

一个面向《明日方舟》基建的效率求解、体系编排与轮换导出引擎。

它解决的不是“把三个最高百分比干员放进一间房”，而是：在玩家实际练度、房间容量、同一干员不能重复上岗、同房互斥、跨设施资源、体系硬核心和跨班绑定同时存在时，仍然给出可解释、可复现的排班。

项目最有价值的部分不是某张固定排班表，而是把基建专家知识拆成可以计算、搜索和回归验证的结构：

- 普通技能由声明式 `EffectAtom` 解释；
- 赤金链、订单分布和单位产出交给专门的领域引擎；
- 难以原子化的固定最优组合由 L3 shortcut 结算；
- 真正的硬约束由编排层保证，其余队友尽量由实际效率自然搜索；
- 贸易、制造、发电分别保留直接效率，不用一个匿名总分掩盖取舍。

## 三分钟理解它能做什么

给定一个练度盒（operbox）和一张基建蓝图，当前引擎可以：

| 能力 | 当前行为 |
|------|----------|
| 单房结算 | 计算贸易、制造、发电和中枢结果；贸易输出纸面效率、单位产出倍率、最终效率与规则 ID |
| 候选搜索 | 在合法池中枚举房间组合，并按各生产域的 `final_efficiency` 排序 |
| 单班编制 | 把 Rule/兼容 System 编译为 `AssignmentPlan`，再 Execute → Fill → Resolve，用全局 `used` 保证一人一岗 |
| 跨设施机制 | 汇总中枢注入、感知信息、人间烟火、木天蓼、虚拟发电等 producer，再让消费房读取 |
| 体系编排 | 表达 required anchor、同房 bond、禁止同房、配方限制、降级与班次绑定 |
| 定时轮换 | 默认生成 αβγ ABC `12h + 6h + 6h`；支持六个具名 profile、普通二/三班 topology 时长请求，以及菲亚/深海专用四班 |
| 账号分析 | 对 operbox 生成练度画像，并与基准盒比较 |
| 导出 | 输出人类可读排班、账号画像 JSON 和 MAA 排班 JSON；core `BaseAssignment` 本身支持 serde |

贸易站是目前分层最完整的领域（L1 + L2 + L3）；制造、发电、中枢、全局资源和编排已经进入同一条全基建主路径，但各自的机制深度并不相同。当前事实地图见 [docs/PROJECT_MAP.md](docs/PROJECT_MAP.md)。

当前数据包含 727 条机制登记、298 个技能定义和 487 个练度实例（282 个逻辑干员）；编排同时使用 6 条声明式 Rule、8 个兼容 registry System 和 4 个中枢制造注入器，另有 16 个贸易 shortcut。仅控制中枢、贸易站、制造站的设施全集就分别对应 `C(55,5)=3,478,761`、`C(77,3)=73,150`、`C(90,3)=117,480` 个原始满房组合。这些数字不是“覆盖率 100%”的营销口号，而是为什么项目必须同时建设机制分层、候选池、Bake 与严格回归的工程尺度。事实入口见 [项目地图](docs/PROJECT_MAP.md)、[性能工程](docs/质量与维护/PERFORMANCE_ENGINEERING.md) 和 [已建模干员](docs/机制/MODELLED_OPERATORS.md)。

## 架构一览

```text
用户当前裁决 → 对应领域 canonical Markdown
                         ↓ 由代码与生成 help 实现和证明
runtime data + operbox + BaseBlueprint
                         ↓
                    compute_plan
                         ↓
               一次用户 rotation probe
                         ↓
              schedule_timed_rotation_request
                         ↓
              assign_shift*（按 profile）
              ├─ build_plan → execute_plan
              ├─ 建池与各设施 fill / search
              └─ 反复 resolve → BaseAssignment
                         ↓
        TeamRotationReport → profile / CLI 文本 / MAA / Worker 响应
```

这里有一条重要边界：shortcut 负责“这个实际组合怎样结算”，不能代替“谁必须进编”；`shift_bind` 负责已入选成员怎样同上同下，也不能代替 required anchor。正因为这些责任被分开，项目才能同时容纳硬体系和自然搜索。

Agent 首先按症状读取 [docs/INDEX.md](docs/INDEX.md) 和生成的 [Agent 路由](docs/生成索引/Agent路由.md)；游戏机制通过 [知识库引用合同](docs/机制/知识库引用.md) 跳转到固定版本的相邻知识库。人类读者仍可从 [docs/指南/OVERVIEW.md](docs/指南/OVERVIEW.md) 和 [docs/指南/GLOSSARY.md](docs/指南/GLOSSARY.md) 开始。

## 直接运行一套真实方案

日常运行只需要能构建本 workspace 的 Rust / Cargo 工具链；Python 仅用于 `scripts/` 下的数据维护。仓库自带各星级最高合法练度的 243 夹具。下面的命令只计算一次用户 rotation，默认使用 αβγ ABC，并从同一结果生成账号画像和 MAA JSON：

```bash
cargo run -q -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --profile-out out/readme-profile.json \
  --maa-out out/readme-maa.json
```

只探测单班自定义布局：

```bash
cargo run -q -p infra-cli -- layout test \
  --layout data/fixtures/243/layout.json \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --text
```

运行机制回归入口：

```bash
cargo run -q -p infra-cli -- verify --all
```

生成并验证本机 Bake 候选表：

```bash
cargo run -q -p infra-cli -- bake all
cargo run -q -p infra-cli -- bake validate
```

`plan`、`layout team-rotation`、`layout eval`、`trade yield` 等完整参数见 [docs/接入与前端/INFRA_CLI.md](docs/接入与前端/INFRA_CLI.md)。

## Rust 开发容器（WSL）

本配置只提供开发工具链，不是生产镜像，不增加 HTTP 服务，也不修改求解器接口。Docker Desktop 使用独立 WSL 2 后端；不在 Ubuntu 中安装 Engine。源码与运行时数据位于外部命名卷 `arknights-v2-source`，挂载到容器 `/workspace`；日常启动和开发不依赖 Ubuntu。修改源码不需要重建镜像。容器默认使用 UID/GID 1000 的普通用户，包含固定 Rust 1.96.0 工具链、Git、C/C++ 构建工具、GDB、Python 3 与 venv。

Windows 只保留启动配置文件：`Dockerfile.dev`（联网构建）、`Dockerfile.dev.offline`（本机离线构建，见下）、`Dockerfile.dev.dockerignore`、`compose.dev.yaml`，放在 Windows 的 `arknights-v2-container` 控制目录。不挂载 Windows 源码目录，也不使用 Ubuntu 路径作为构建上下文。Compose 项目名固定为 `arknights-v2-dev`，避免移动控制目录时意外创建另一套缓存卷。

首次准备需要：

1. 安装/启动 Windows Docker Desktop，启用 WSL 2 engine 与 Linux containers；无需启用 Ubuntu WSL Integration。
2. 一次性从原 Ubuntu 仓库复制源码、运行数据、contracts、脚本、文档和 `.git` 到 `arknights-v2-source`，保留已修改及未跟踪的工作文件；不只 clone 已提交的 HEAD。原仓库保留，不删除、不移动。编译缓存、运行证据及密钥是否复制需分别确认；迁移后对比文件内容、Git 状态和 UID/GID。
3. 源码卷应对 developer（默认 UID/GID 1000）可写。单独的空卷并不代表迁移成功；确认 `/workspace/Cargo.toml` 和预期数据存在后才开始构建。

源码卷声明 `external: true`，不会由 Compose 自动创建或随 `down -v` 删除；首次迁移需显式创建并填充它。Windows PowerShell 在控制目录执行：

```powershell
# NUL 是 Windows 空设备，避免 Compose 自动读取 .env。
docker compose --env-file NUL -f compose.dev.yaml up -d --build
docker compose --env-file NUL -f compose.dev.yaml exec rust-dev bash
```

Windows Git Bash 调用 Windows `docker.exe` 时也可使用 `NUL`；如设置 `MSYS_NO_PATHCONV=1`，不可用 `/dev/null`，否则可能被当作不存在的 Windows 文件。编辑源码推荐在支持 Dev Containers 的编辑器中选择“Attach to Running Container”，连接本配置的 `rust-dev` 容器并打开 `/workspace`；不要在宿主机直接编辑 Docker 内部磁盘或把 `docker-desktop` 当成开发发行版。

进入容器后：

```bash
cargo build --release --locked -p infra-cli
"$CARGO_TARGET_DIR/release/infra-cli" --help
# 一次真实求解；输出写入源码卷内的 out/。
"$CARGO_TARGET_DIR/release/infra-cli" plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --profile-out out/container-profile.json \
  --maa-out out/container-maa.json \
  --assert-invariants
# 文档维护检查不依赖模型/向量库。
python3 scripts/codex/generate_agent_index.py --check
python3 scripts/codex/docs_inventory.py --check
python3 scripts/codex/check_repository_facts.py
```

Cargo 下载缓存与构建产物分别保存在 Compose 命名卷中。容器的 `CARGO_TARGET_DIR=/home/developer/target`，**不是**仓库的 `target/`，不混用 WSL 宿主机的既有二进制；批量验收或依赖默认 target 路径的脚本应显式传入 `--binary "$CARGO_TARGET_DIR/release/infra-cli"`。本项目按 `AGENTS.md` 使用 release、真实 CLI、终态断言和文档检查，不运行源码测试。

Worker 是 stdin/stdout JSONL，需关闭 TTY；从 Windows PowerShell 验证握手：

```powershell
'{"id":"dev-ping","method":"ping"}' | docker compose --env-file NUL -f compose.dev.yaml exec -T rust-dev /home/developer/target/release/infra-cli serve
```

如需 Python 第三方依赖，在容器内创建项目虚拟环境后按实际脚本需求安装；本镜像不自动安装 `pyproject.toml` 中的模型/向量库重依赖，也不执行数据同步、Bake 或外部部署。默认不挂载宿主机 SSH、AWS、Docker socket，不提供特权或端口。**容器中的源码、Git 和输出保存在源码卷，不会自动同步回原 Ubuntu 仓库。若把 `.env`、`.aws` 等文件导入源码卷，容器同样能读取它们；命名卷不是密钥保险箱。** 源码卷现在是开发真源，未提交工作应定期导出到 Windows 独立备份文件，Git 提交也只存在本地卷，不能代替异地备份；重置/卸载 Docker Desktop 可能丢失卷。 构建专用忽略规则使这些文件不会发送到镜像构建上下文。

停止并移除开发容器但保留源码和编译缓存（Windows PowerShell）：

```powershell
docker compose --env-file NUL -f compose.dev.yaml down
```

不要随意加 `-v`，否则会删除本配置的 Cargo 命名卷缓存（外部源码卷不会随此命令删除）。修改工具链、系统包或 UID/GID 后需重建镜像；已有卷的所有权不会随 UID/GID 自动迁移，迁移前应先核对并保留所需缓存。

### 容器内的 Codex 与 RTK

镜像固定安装 Codex CLI 0.166.0 与 RTK 1.1.0，二者都不提交进仓库。`~/.codex` 与 `~/.config/rtk` 各由独立命名卷持久化，重建容器不丢配置与统计。

模型调用链是 Codex → 本机清洗代理（`127.0.0.1:13427`）→ Magpie 网关（`127.0.0.1:13425`）。容器内通过 `host.docker.internal` 访问；这两个服务是 Windows 上的前台进程，不是常驻服务，容器启动前需要先在两个 PowerShell 窗口分别运行 `tools/start-magpie-windows.ps1 -Service gateway` 和 `-Service sanitize`。

Windows 上的 Magpie 不能占用 3425/3427：`.wslconfig` 启用了 `networkingMode=mirrored`，WSL 内的 Magpie 已经占住这两个 Windows 本地端口。换端口后链路行为一致，只有端口号与原 WSL 不同。

Codex 自带沙箱依赖 bwrap。Docker 默认 seccomp 禁止创建用户命名空间，因此 Compose 设置了 `security_opt: seccomp=unconfined`；否则容器内任何命令都会以 `bwrap: No permissions to create a new namespace` 失败。

RTK 通过 `~/.codex/hooks.json` 的 `PreToolUse` 调用 `rtk hook codex`，首次运行需要在交互会话里批准一次 hook 信任；`codex exec` 无人值守时需要显式加 `--dangerously-bypass-hook-trust`。`rtk gain` 可查看压缩统计。

本机 Docker Hub 元数据与 apt 均超时，因此 `compose.dev.yaml` 指向 `Dockerfile.dev.offline`：Rust 层使用本地标签 `arknights-v2-dev-base:rust-1.96.0`，Codex/RTK 使用 `vendor/` 内的归档（校验值在 `vendor/SHA256SUMS`）。网络正常时改用 `Dockerfile.dev` 可从上游重建。`vendor/` 体积较大，不提交。

进入本项目的 Codex（一条命令，脚本自己起代理、起容器、退出时收代理）：

```powershell
& "C:/Users/KnightCode/WorkBuddy/2026-11-04-18-23-50/arknights-v2-container/codex-dev.ps1"
```

常用变体：

```powershell
# 传参给 codex
& ".../codex-dev.ps1" exec --ephemeral "检查 git status"
# 只要容器 shell，不进 Codex
& ".../codex-dev.ps1" -Shell
# Codex 退出后保留代理
& ".../codex-dev.ps1" -KeepStack
# 停掉所有代理
& ".../codex-dev.ps1" -Stop
```

脚本会检查 13425/13427 是否已在监听，已在则复用；Docker 引擎没起会自动拉起；容器已存在则复用。若后台启动失败（例如 PowerShell 环境变量异常），会打印需要手动执行的那一条命令。

Windows PowerShell 5.1 可能同时存在 `Path`/`PATH`/`path` 三个键，导致 `Start-Process` 报 `Item has already been added`。脚本启动时会先合并这三个键，避免该问题。

手动分窗口启动仍然可用（调试代理本身时更方便）：

```powershell
& ".../tools/start-magpie-windows.ps1" -Service gateway
& ".../tools/start-magpie-windows.ps1" -Service sanitize
```

### SSH 密钥

容器需要 SSH 客户端才能用 `git@` 远端；镜像已装 `openssh-client`。密钥放在独立命名卷 `arknights-v2-ssh`，挂到 `/home/developer/.ssh`，权限为目录 700、私钥 600。

密钥按目标主机 IP 命名：`114.66.55.78_id_rsa`、`114.66.55.78_ed25519`（含 `.pub`）。`110.42.33.126_id_rsa` 与 `211.161.232.77_id_rsa` 是指向 `114.66.55.78_id_rsa` 的符号链接，因为这三个主机共用同一把私钥；`config` 的 `IdentityFile` 已同步为新名字。`id_rsa` 保留为兼容链接，`scripts/export_quality_feedback.py` 等按旧名引用密钥的脚本不受影响。原 WSL 与 Windows 的 `~/.ssh` 均未改动。

**这是新增的一份私钥副本**：任何能执行 `docker` 或进入容器的人都能读到它。不要在共享机器上保留该卷；不再需要时用 `docker volume rm arknights-v2-ssh` 删除（容器需先停止）。

### 代理常驻（登录自启）

Magpie 与清洗代理不在容器里，也不由编辑器管理。日常可用 `codex-dev.ps1` 临时拉起，但它退出时会收掉代理。

需要开机即用（尤其是随时打开 Magpie Web UI）时，注册一个当前用户级的计划任务：

```powershell
$name = 'ArknightsV2 Magpie Stack'
$script = 'C:/Users/KnightCode/WorkBuddy/2026-11-04-18-23-50/tools/start-agent-stack-hidden.ps1'
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$script`""
$trigger = New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME"
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $name -Action $action -Trigger $trigger -Principal $principal
```

- 手动触发：`Start-ScheduledTask -TaskName 'ArknightsV2 Magpie Stack'`
- 查看状态：`Get-ScheduledTask -TaskName 'ArknightsV2 Magpie Stack'`
- 取消自启：`Unregister-ScheduledTask -TaskName 'ArknightsV2 Magpie Stack' -Confirm:$false`
- 运行日志：`private/windows-proxy-runtime/autostart.log`

脚本只补起未在监听的端口，重复执行不会起第二份；`codex-dev.ps1` 检测到端口已监听时也只复用、退出时不会停掉自启的实例。

验证状态（2026-11-04）：已安装并启动 Docker Desktop 4.98.0（当前用户安装，WSL 2 后端）；Windows 控制目录为 `C:/Users/KnightCode/WorkBuddy/2026-11-04-18-23-50/arknights-v2-container`。已复制源码、Git 与未提交改动（含用户明确批准的 `.env`/`.aws`），未复制根 target、虚拟环境和 Python 缓存；原 Ubuntu 项目保留。镜像构建、普通用户卷权限、容器内 release 构建、Worker ping（v1–v7）通过。归档流 SHA256 核对 37908 个文件无内容差异（排除可变 Git index），但核对期间原 `.git` 目录变化导致 tar 非零退出，不宣称严格冻结快照。Agent 路由和文档清单检查通过；repository facts 检查因历史 Ubuntu 绝对链接及未迁移 target 证据失败，未改写历史文档。完整 plan 求解、批量终态验收和 GDB 调试尚未运行；不修改机制，不新增或运行源码测试。后续开发只在卷内进行，原 Ubuntu 仓库不会自动同步。

## 正确性优先，也认真对待性能

组合搜索会很快膨胀，但本项目不会用不透明剪枝换取一个看似漂亮的答案。

当前性能策略包括：

- 贸易等适用领域使用工具人池和 role policy 缩小候选边界；普通排班制造仍搜索全部合法普通制造候选；
- 单房组合求值可用 Rayon 并行，跨房则按稳定生命周期提交并重新 `resolve`；
- Bake 可生成 schema v13 的 3/2/1 人单房候选索引；运行时只有在数据指纹、布局、练度和动态上下文都兼容时才读取；仓库内旧 catalog 失配时会安全回退；
- Bake 不兼容、缺失或过期时回退到实时 solver。缓存失效只应影响速度，不能改变语义；
- `Efficiency` 用千分整数保存和排序，避免浮点尾差改变候选次序。

当前单班编制仍是领域约束下的分阶段、逐房搜索，不宣称全基建整数规划意义上的全局最优。动态贸易 producer 的统一联合搜索与完整候选列 Bake（A+）是**未来计划**，尚不是当前能力；设计边界见 [docs/TODO/详情/性能质量/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md](docs/TODO/详情/性能质量/DYNAMIC_PRODUCER_BAKED_SEARCH_PLAN.md)。

## 为什么结果值得审计

项目把“可信”做成了可检查的工程链路：

1. 当前用户明确裁决优先；稳定业务语义由对应领域的唯一 canonical Markdown 拥有。
2. 代码和生成 help 证明当前实现事实；JSON、CSV、fixture 和 Bake 只承载或核对实现。
3. `skill_table.id == buff_id`，干员与技能归属只在 `operator_instances.json` 维护。
4. L1 解释器只认 `buff_id`，不会按干员中文名偷偷改公式。
5. 生产域统一输出三位小数直接效率；机制等效效率只作解释，不会重复乘入产出。
6. CSV 回归锚点、单位产出锚点、终端不变量和真实 CLI 夹具覆盖不同责任层；源码测试不参与当前验证。
7. Debug 流程要求先复现、定位生命周期边界、删除冲突旧路径、补不变量回归，并保留可点击验证日志和 JSON 产物。

这不意味着仓库永远没有 bug；它意味着一个结果可以追到文档口径、数据条目、解释阶段、搜索边界、回归断言和实际 CLI 输出，而不是只能相信一张最终截图。

## 从哪里开始读

| 你是谁 / 想做什么 | 入口 |
|-------------------|------|
| 第一次了解项目 | [docs/指南/OVERVIEW.md](docs/指南/OVERVIEW.md) → [docs/指南/GLOSSARY.md](docs/指南/GLOSSARY.md) |
| Agent 按症状定位问题 | [Agent 路由](docs/生成索引/Agent路由.md) → `docs/调用链/` → 对应模块与机制 owner |
| 已知问题属于哪个领域 | [docs/INDEX.md](docs/INDEX.md) → 七个领域入口 `README.md` → canonical |
| 想沿一次真实请求看代码 | [docs/调用链/plan请求.md](docs/调用链/plan请求.md)、[docs/EXAMPLES/243_FULL_E2.md](docs/EXAMPLES/243_FULL_E2.md) |
| 懂基建策略，不关心代码 | [docs/指南/GONGSUN_RUNTIME_OVERVIEW.md](docs/指南/GONGSUN_RUNTIME_OVERVIEW.md) |
| 想运行 CLI 或接前端 | [docs/接口/CLI与Worker.md](docs/接口/CLI与Worker.md)、[docs/接口/输入输出与兼容.md](docs/接口/输入输出与兼容.md) |
| 想判断结果或性能是否可信 | [docs/质量与维护/QUALITY_AND_AUDIT.md](docs/质量与维护/QUALITY_AND_AUDIT.md)、[docs/质量与维护/PERFORMANCE_ENGINEERING.md](docs/质量与维护/PERFORMANCE_ENGINEERING.md) |
| 想理解机制映射 | [docs/机制/知识库引用.md](docs/机制/知识库引用.md) → [输入与技能映射](docs/模块/输入与技能映射.md) |
| 想理解编排与轮换 | [排班与轮换](docs/模块/排班与轮换.md)、[定时轮换调用链](docs/调用链/定时轮换.md) |
| 想修 bug / 审计体系 | [AGENTS.md](AGENTS.md)、[错误与诊断](docs/接口/错误与诊断.md)、[docs/质量与维护/QUALITY_AND_AUDIT.md](docs/质量与维护/QUALITY_AND_AUDIT.md) |
| 想查文件和模块 | [docs/INDEX.md](docs/INDEX.md)、[docs/PROJECT_MAP.md](docs/PROJECT_MAP.md) |
| 想查当前开放工作 | [TODO 总板](docs/TODO/README.md) |
| 想追溯 ADR、完成记录或旧方案 | [决策与历史入口](docs/INDEX.md#4-当前工作与历史) |

## 明确的非目标

- 宿管自动分配与完整心情恢复规划；
- 根据 ETA 自动改写所有班次时长；
- 全基建连续时间最优化；
- 对所有房间、所有班次做无界全局组合搜索；
- 用 CLI、fixture 或下游特判承载游戏机制；
- 把贸易、制造和发电揉成不可解释的匿名总分。

## 仓库结构

```text
crates/infra-core/   机制、搜索、编排、排班、导出
crates/infra-cli/    参数、加载、输出、回归命令
data/                运行时模型、体系、shortcut、夹具和锚点
docs/                当前事实、领域语义、维护流程、TODO 与归档
scripts/             数据构建与审计脚本
```

## License

MIT
