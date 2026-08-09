# ADR 0003：办公室与会客室前端结果契约

> 文档角色：decision
> 生命周期状态：accepted
> 当前真源：docs/FRONTEND_CLI.md
> 摘要：规定办公室与会客室静态求值未来如何通过 plan.compute 增量提供给 beta 前端

> 历史决策状态：accepted
> 日期：2026-07-19
> 关联实现：核心提交 `b822c82`；前端仓库 <https://github.com/KnightCodeSquareMatrix/ArknightsInfraCalc-v2_beta_test_frontend>

## 当前实现状态

本 ADR 的支持设施前端扩展已接受，但尚未实现。核心提交 `b822c82` 只提供静态求值基础；当前 `plan.compute` v1 的 `result.rotation.shifts[]` 不含 `support_facilities`，前端类型和 UI 也尚未消费该字段。2026-07-23 完成的 Worker 内联 JSON 迁移只替换 transport 和 Plan 编排，明确未包含本 ADR 的产品扩展。

因此下文定义的是后续实现必须遵守的增量契约，不是当前发布能力声明。当前 Worker 和发布状态仍以 [FRONTEND_CLI.md](../FRONTEND_CLI.md) 为准。

## 背景

核心仓库已经实现办公室、会客室显式编制的有界静态求值：

- `ResolvedBase.office_rooms` / `meeting_rooms` 保存房间结果；
- `infra-cli layout eval` 的 JSON 已输出 `office` / `meeting`；
- 概率、线索事件和交流状态不算分，通过 `ignored` 报告；
- 当前不能安全表达的跨中枢、宿舍和派系条件不算分，通过 `unsupported` 报告。

beta 前端不调用 `layout eval`。当前 worktree 在 `src/server/infra.ts` 中向长驻 `infra-cli serve` 发送 `plan.compute`，再从 `result.rotation.shifts[].efficiencies.room_lines` 提取贸易、制造和发电结果。`src/types.ts` 的 `RotationShift` 也只声明这三个生产域。因此核心提交 `b822c82` 不会破坏当前前端，但新结果也不会自动出现在页面。

办公室与会客室的收益量纲不同于贸易、制造和发电：办公室速度影响公招刷新次数获取，会客室速度影响线索搜集。当前模型只输出技能速度加成，不输出设施基础速度、实际公招券/黄票收益、线索概率或信用收益。前端不得把这些值加进现有生产总分。

## 决策

### 1. 复用现有 `/api/plan` 与 `plan.compute` 请求

前端继续使用现有浏览器 `/api/plan` 请求和 `infra-cli serve` 的 `plan.compute` 方法，不新增第二个求解请求，也不要求浏览器提交显式 assignment。支持设施字段挂在现有 `result.rotation.shifts[]` 中，不恢复 legacy 路径协议。

后续实现应在每个班次的 shift 对象中增加可选字段 `support_facilities`：

```json
{
  "index": 0,
  "duration_hours": 12,
  "efficiencies": {
    "trade_efficiency": 0,
    "manufacture_efficiency": 0,
    "power_efficiency": 0,
    "room_lines": []
  },
  "support_facilities": {
    "office": [
      {
        "room_id": "office",
        "autofill": false,
        "result": {
          "facility": "office",
          "skill_speed_bonus_pct": 35.0,
          "external_speed_bonus_pct": 0.0,
          "total_speed_bonus_pct": 35.0,
          "meeting_speed_inject_pct": 10.0,
          "mood": [
            { "operator": "乌有", "delta_per_hour": 0.0 }
          ],
          "contributions": [
            {
              "row_id": 177,
              "buff_id": "hire_spd&clue[101]",
              "operator": "乌有",
              "skill": "好事之徒",
              "kind": "speed_flat",
              "value": 35.0,
              "active": true
            }
          ],
          "ignored": [],
          "unsupported": []
        }
      }
    ],
    "meeting": [
      {
        "room_id": "meeting",
        "autofill": true,
        "result": null
      }
    ]
  }
}
```

`office` 和 `meeting` 保持数组，与蓝图房间结果形式一致；当前游戏蓝图最多各一间，但协议不把该事实编码成单对象。

### 2. 字段是向后兼容的可选扩展

- 完成本 ADR 的新核心可以在 shift 中提供 `support_facilities`。
- 当前 v1 与其他尚未实现本 ADR、但支持 `plan.compute` v1 的兼容核心都没有该字段；前端必须把它声明为可选。
- 前端收到字段缺失时不显示支持设施结果，不报错、不伪造零值。
- `autofill: true` 时 `result` 必须为 `null`，前端显示“自动填充，未计算”。
- `autofill: false` 时 `result` 必须存在。

本次不改变 `/api/plan` 请求体、`PlanApiResponse` 顶层结构、MAA schema、profile schema 或现有生产效率字段。

### 3. 按真实班次时长求值

核心把结果接入 `TeamShiftResult` 前，必须以该 shift 的 `duration_hours` 求时间爬升平均值，不能沿用 `resolve_base` 当前固定的 24 小时口径。办公室先求值，其确定性会客室注入再进入同一班会客室结果。

前端只展示核心返回值，不重新计算时间爬升、招募位、全局资源或跨房间加成。

### 4. 前端展示语义

前端将支持设施结果放在对应班次内，与贸易、制造和发电房间并列展示，但不参与 `trade_score`、`manu_prod_sum`、`power_charge_sum` 或现有每日生产汇总。

最小展示要求：

- 办公室显示“公招联络技能速度加成”；
- 会客室显示“线索搜集技能速度加成”；
- 主值使用 `total_speed_bonus_pct`；
- 可展开查看房内 `skill_speed_bonus_pct` 和外部 `external_speed_bonus_pct`；
- `ignored` 表示按产品裁决不计算的概率/事件效果；
- `unsupported` 表示当前实现缺口，至少显示数量和原因；
- `contributions.active: false` 的条件效果不得显示为已生效加成；
- 不把 `total_speed_bonus_pct` 标成“最终游戏倍率”或折算为抽卡数、黄票、信用收益。

### 5. 前端类型契约

前端在 `src/types.ts` 增加等价类型，并把字段挂到 `RotationShift`：

```ts
export interface SupportContribution {
  row_id: number;
  buff_id: string;
  operator: string;
  skill: string;
  kind: string;
  value: number;
  active: boolean;
}

export interface SupportNotice {
  row_id: number;
  buff_id: string;
  operator: string;
  reason: string;
}

export interface SupportRoomResult {
  facility: "office" | "meeting";
  skill_speed_bonus_pct: number;
  external_speed_bonus_pct: number;
  total_speed_bonus_pct: number;
  meeting_speed_inject_pct: number;
  mood: Array<{ operator: string; delta_per_hour: number }>;
  contributions: SupportContribution[];
  ignored: SupportNotice[];
  unsupported: SupportNotice[];
}

export interface ResolvedSupportRoom {
  room_id: string;
  autofill: boolean;
  result: SupportRoomResult | null;
}

export interface RotationShift {
  // existing fields remain unchanged
  support_facilities?: {
    office: ResolvedSupportRoom[];
    meeting: ResolvedSupportRoom[];
  };
}
```

`rotationShiftsFromServe` 当前只规范化已知生产字段，不保留 `support_facilities`。实现本 ADR 时必须显式规范化并保留该字段，拒绝非数组房间列表和非有限数值，不能仅靠 TypeScript 类型断言信任进程输出。

## 两仓责任

### 核心仓库

1. 将每班办公室、会客室求值结果挂到 `TeamShiftResult.support_facilities`。
2. 使用班次真实 `duration_hours`，不使用固定 24 小时。
3. 保持 `efficiencies` 和每日生产汇总不变。
4. 为字段存在、旧字段不变、autofill 和 6/12 小时时间爬升增加 `plan.compute` 实测。
5. 更新 `FRONTEND_CLI.md` 的 serve 响应契约。

### beta 前端仓库

1. 在 `src/types.ts` 增加上述可选类型。
2. 在 `src/server/infra.ts::rotationShiftsFromServe` 规范化并保留支持设施结果。
3. 在班次房间 UI 中显示办公室和会客室结果、autofill、ignored、unsupported。
4. 兼容 `plan.compute` v1 但没有该字段的 CLI 保持当前页面行为。
5. 不修改 `/api/plan` 请求、MAA 下载和生产效率总分。

前端可以先合并兼容类型和 UI 空状态，但在核心完成 serve 输出前，不得用 fixture 值冒充真实运行结果。

## 验收条件

### 协议

- 使用当前 v1 或其他尚未实现本 ADR、但支持 `plan.compute` v1 的核心：`support_facilities` 缺失，前端计划生成与当前一致。
- 使用完成本 ADR 的新核心：每个 shift 可选携带办公室、会客室数组。
- 空房：`autofill=true` 且 `result=null`。
- 已求值房间：主值、贡献、忽略项和未支持项可见。
- 原有 `efficiencies`、`scores`、MAA 和 profile 输出不发生字段删除或改名。

### 核心

- 6 小时和 12 小时伊内丝结果分别按各自时长计算平均值。
- 乌有三级办公室向同班会客室注入 10%。
- `plan.compute` 的实际 JSON 在 `result.rotation.shifts[]` 中包含本 ADR 定义的结构。

### 前端

- `npm run build` 和既有测试通过。
- 缺失字段、autofill、正常结果、含 `ignored`、含 `unsupported` 五种 fixture 均可渲染。
- 支持设施数值不会进入贸易、制造、发电分数或每日汇总。

## 后果

### 正面

- 复用现有长驻 serve 通道，不增加额外进程或请求生命周期。
- 未提供和已提供 `support_facilities` 的 `plan.compute` v1 核心可渐进兼容。
- 支持设施保持独立量纲，避免把公招刷新、线索和生产效率错误相加。
- 前端能忠实展示模型边界，而不是隐藏未支持效果。

### 代价

- 核心必须把支持设施求值从固定 24 小时改为班次时长。
- 前端需要增加一组类型、规范化和展示组件。
- 在自动填充房间中没有实际人员时，只能报告未求值，不能给出估算值。

## 明确不做

- 本 ADR 不规定办公室或会客室自动选人（核心已于 2026-08-06 裁决：会客室在单班流水线末尾自动填满两人，参与 ABC 轮换，见 `EFFECT_ATOM_DESIGN.md` §十；办公室仍为显式编制）。
- 不模拟线索概率、线索交流或必得线索事件。
- 不把办公室速度折算为抽卡数、绿票或黄票。
- 不增加支持设施每日加权总分。
- 不改变 MAA 排班文件格式。
