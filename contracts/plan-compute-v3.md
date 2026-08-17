# `plan.compute` 参数契约 v3

> 状态：已发布、不可变  
> 机器契约：[plan-compute-v3.schema.json](plan-compute-v3.schema.json)  
> JSON Schema 方言：Draft 2020-12  
> 实现入口：`infra-cli serve` / `method: "plan.compute"`

## 1. 契约范围

本文解释 `plan.compute` 请求中的 `params` 对象。JSON Schema 是结构 admission 的机器真源；本文不得定义与 schema 不同的字段、类型或范围。

本契约不覆盖：

- NDJSON 请求 envelope 的 `id`、`method`；
- 成功响应中的 profile、rotation 和 MAA；
- 错误 envelope；
- legacy `method: "plan"` 路径协议。

上述协议见 [前端对接说明](../docs/FRONTEND_CLI.md) 和 [Serve 指南](../docs/FRONTEND_SERVE_GUIDE.md)。

## 2. 参数结构

```json
{
  "schema_version": 2,
  "layout": {
    "template": "243",
    "drone_cap": 235,
    "scenario": {},
    "rooms": [
      {
        "id": "control",
        "kind": "control_center",
        "level": 3
      }
    ]
  },
  "operbox": [
    {
      "id": "char_009_12fce",
      "name": "12F",
      "elite": 0,
      "level": 30,
      "own": true,
      "potential": 6,
      "rarity": 2
    }
  ],
  "labels": {
    "layout": "243",
    "operbox": "Full tier"
  },
  "options": {
    "rotation": "abc_12_6_6",
    "top": 20,
    "system_preferences": {},
    "maa_title": "My schedule"
  }
}
```

该示例只展示通过结构 schema 的最小形状，不保证单房布局可求解。真实请求应提交完整 layout 和 operbox，例如 `data/fixtures/243/` 中的标准输入。

### 顶层字段

| 字段 | 类型 | 必填 | 约束 |
|---|---|---|---|
| `schema_version` | integer | 是 | 固定为 `2` |
| `layout` | object | 是 | 见 BaseBlueprint |
| `operbox` | array | 是 | 1 至 1000 项 |
| `labels` | object | 否 | 省略时使用内联输入的默认标签 |
| `options` | object | 否 | 省略时使用默认求解选项 |

顶层及所有具名 wire 对象均设置 `additionalProperties: false`。未知字段会在 Serde 反序列化前被拒绝。

## 3. BaseBlueprint

| 字段 | 类型 | 必填 | 默认值 / 约束 |
|---|---|---|---|
| `template` | string 或 null | 否 | 展示标签，不参与路径解析 |
| `drone_cap` | integer | 否 | 默认 `135`，范围 `0..=4294967295` |
| `scenario` | object | 否 | 默认 `{}` |
| `rooms` | array | 是 | 1 至 64 间 |

### `scenario`

| 字段 | 类型 | 说明 |
|---|---|---|
| `elite_facility_count` | integer 或 null | `u8` 范围 |
| `sui_facility_count` | integer 或 null | `u8` 范围 |
| `dorm_occupant_count` | integer 或 null | `u8` 范围 |
| `base_workforce` | string[] | 默认空数组 |
| `initial_global` | object | key 必须是 schema 列出的全局资源 ID，value 为 number |

### `rooms[]`

| 字段 | 类型 | 必填 | 约束 |
|---|---|---|---|
| `id` | string | 是 | 同一 layout 内必须唯一 |
| `kind` | enum | 是 | 见下方设施枚举 |
| `level` | integer | 是 | JSON 结构为 `u8`；领域校验要求至少为 1 |
| `product` | object 或 null | 否 | 贸易站和制造站必须提供匹配类型 |
| `dorm_beds` | integer 或 null | 否 | `u8`；宿舍提供时不得为 0 |
| `dorm_ambience_level` | integer 或 null | 否 | `u8`；宿舍提供时不得为 0 |

设施枚举：

```text
control_center | trade_post | factory | power_plant | dormitory |
office | meeting_room | training_room | workshop
```

生产配置：

```json
{ "trade": { "order": "gold" } }
```

贸易订单为 `gold | originium`。

```json
{ "factory": { "recipe": "gold" } }
```

制造配方为 `all | battle_record | gold | originium`。

## 4. OperBox

每项必须包含全部字段：

| 字段 | 类型 | 约束 |
|---|---|---|
| `id` | string | 非空；数组内唯一 |
| `name` | string | 非空；数组内唯一 |
| `elite` | integer | `0..=2` |
| `level` | integer | `1..=90` |
| `own` | boolean | `false` 时不参与排班 |
| `potential` | integer | `1..=6` |
| `rarity` | integer | `1..=6` |

Worker 会先 trim `id` 和 `name`，再检查空值及重复项。
`rarity`、`elite` 与 `level` 还必须构成游戏中可达的练度：1–2 星最高精 0 / 30，3 星最高精 1 / 55，4 星最高精 2 / 70，5 星最高精 2 / 80，6 星最高精 2 / 90。未知星级或不可能练度直接失败，不会自动修正或降档。

## 5. Labels 与 Options

### `labels`

`layout` 和 `operbox` 均为 string 或 null。有效非空标签最多 200 UTF-8 bytes；该字节约束由 Rust 校验，schema 使用 `x-maxUtf8Bytes: 200` 记录。

### `options`

| 字段 | 类型 | 默认值 / 约束 |
|---|---|---|
| `rotation` | enum | 默认 `abc_12_6_6`；v2 新增 `abc_12_12_12`（36h ABC 三班） |
| `top` | integer | 默认 `20`，范围 `1..=100` |
| `system_preferences` | object | 默认 `{}`；value 必须为 string |
| `maa_title` | string 或 null | 可选 MAA 标题 |
| `fiammetta_enable` | boolean | 默认 `true`；`false` 时禁用菲亚梅塔回岗，且与 `rotation: fiammetta_8_8_4_4` 组合直接失败 |

rotation 枚举：

- `abc_12_12_12`：ABC `12/12/12`，与 `abc_12_6_6` 共用三队轮休结构，总循环 36h；日效率按 36h 周期归一化。

```text
abc_12_6_6 | abc_12_12_12 | main_backup_12_12 |
fiammetta_8_8_4_4 | abyssal_7_5_7_5
```

JSON Schema 中的 `default` 是契约注解；实际默认值由 Worker 的 Serde DTO 提供。

## 6. 校验顺序

Worker 按以下顺序处理：

1. NDJSON frame、UTF-8 和 8 MiB 上限；
2. 请求 envelope 与 method 分发；
3. 本契约的 JSON Schema admission；
4. Serde 反序列化；
5. layout、operbox、label 和编排偏好的领域校验；
6. 求解与结果序列化。

JSON Schema 负责字段集合、基础类型、枚举、externally-tagged product 结构及数值范围。以下规则由领域校验负责：

- room ID、operator ID 和 operator name 唯一性；
- room kind 与 product 的跨字段一致性；
- office / meeting room 数量上限；
- trim 后非空和 UTF-8 字节上限；
- `system_preferences` 中 rule / alternative 是否存在。

当前 schema admission 或领域校验失败均返回 `PLAN_FAILED`，stage 为 `plan.compute`。错误码细分不属于本契约。

## 7. 版本与发布

v2 在 v1 基础上增加 `abc_12_12_12` profile；v3 在 v2 基础上增加 `options.fiammetta_enable`（默认 `true`，允许调用方显式禁用菲亚梅塔回岗）。后续请求字段或语义变化仍必须发布新的 schema 版本，并使用新的 `schema_version`。

Worker 的 `ping` result 包含：

```json
{
  "protocol_version": 1,
  "plan_schema_version": 1,
  "supported_plan_schema_versions": [1, 2, 3],
  "plan_contract_sha256": "<64 lowercase hex characters>",
  "plan_contract_sha256_by_version": {"1":"<sha256>","2":"<sha256>","3":"<sha256>"},
  "solver_executable_sha256": "<64 lowercase hex characters>",
  "solver_git_commit": "<full Git commit>",
  "solver_built_at": "<UTC RFC3339 timestamp>"
}
```

每个 Worker 响应顶层都包含本次运行的 solver 构建快照：

```json
{
  "solver": {
    "git_commit": "<full Git commit or null>",
    "built_at": "<UTC RFC3339 timestamp or null>"
  }
}
```

`plan.compute` 的成功和失败响应都包含该快照。前端反馈必须使用对应 `plan.compute` 响应中的快照，不使用最近一次 `ping`。`solver_git_commit` 和 `solver_built_at` 由构建时自动注入；运行时不读取 Git。若构建环境无法提供元数据，字段为 `null`，调用仍可继续。

`plan_contract_sha256` 为兼容旧调用方继续对应 v1；`plan_contract_sha256_by_version` 分别记录 Worker 内嵌 v1/v2/v3 JSON Schema 原始 bytes 的 SHA-256。Linux 发布包在 `contracts/SHA256SUMS` 中记录各版 schema 和文档的校验值。`solver_executable_sha256` 是可执行文件字节的 SHA-256，仅作为制品完整性诊断，不是 solver 版本身份。运行时可从外部目录加载数据，因此上述字段都不覆盖外部数据身份。

**能力判定规则**：v1 调用方继续以 `protocol_version=1`、`plan_schema_version=1` 判断兼容；需要 v2 的调用方还必须确认 `supported_plan_schema_versions` 包含 `2`；需要 `fiammetta_enable` 的调用方还必须确认 `supported_plan_schema_versions` 包含 `3`。所有 hash 和构建元数据只作诊断。
