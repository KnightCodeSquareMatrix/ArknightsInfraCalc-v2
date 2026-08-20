# Production Feedback Closure Audit

> 文档角色：evidence
> 生命周期状态：closed
> 当前真源：docs/MAINTENANCE_MODE.md
> 摘要：记录已关闭生产反馈批次的证据和回归矩阵

> Scope: production beta/test feedback after launch.
> Tracking started: 2026-07-03.
> Closure correction: 2026-07-03. The feedback bugs are already fixed in code; this file now records closure coverage and the smoke matrix that should keep them fixed.
> Raw folders are evidence; do not edit bundled JSON while auditing.
> 2026-07-18 review: bounded office/reception static evaluation does not change this closure matrix.

## Current State

All 27 imported feedback cases are treated as closed or covered by a sibling case. The remaining work is not feature development; it is keeping the closure evidence easy to find.

| State | Count | Meaning |
|-------|------:|---------|
| `closed` | 23 | Fixed or made expected by current code/docs/tests |
| `duplicate-covered` | 4 | Same root as another closed case; retained as evidence |
| `open` | 0 | No known unresolved production feedback item in this batch |

## Local Evidence Digests

<!-- BEGIN GENERATED LOCAL EVIDENCE -->
| ID | Bundle SHA-256 | Availability |
|---|---|---|
| FB-20260627-085944 | `d0aa7611c46fad8014d19a2b7f951597ee5f0f857853921601f53691e4904cc4` | `local-only` |
| FB-20260627-090047 | `0d98c5d53c72e8e06a5242c295bf550b1c5cff4f401269f9aafbbd5466e8c9e0` | `local-only` |
| FB-20260628-131551 | `79cd2143c867b3948796968c031958bce0386f6ab4e234159595900c0f17056b` | `local-only` |
| FB-20260628-132114 | `076646e7566a8b208771eae1007381d4ed73badfad7f2c816dd8bb84182bef8a` | `local-only` |
| FB-20260628-160142 | `904d92acaf8f81805f2797ec65c36ee1db7191df0ac901767a3aa4e7e6e8f491` | `local-only` |
| FB-20260628-160411 | `2829a17bb018ab00459f66cf9854d1e86a21a4a84b3eb8fd27f447fad04ad953` | `local-only` |
| FB-20260628-162229 | `c3a05eb40dbfb5a979df807027ebc79c40c2d84e0467d47471cfcb92eedf0cf3` | `local-only` |
| FB-20260628-162304 | `5b352288b37d98ed09b0791374062bfc02be3f8b87261e4b71644c76173c22ae` | `local-only` |
| FB-20260628-170234 | `cf755bd73b2ada70ab60ffbfbd98654d9315c3d27d5ba0684488373d97dec0e9` | `local-only` |
| FB-20260628-170245 | `f7b67c2397e59330a4d1439d5d6ae37c0b7c36f1dd5692027599a89601a72e01` | `local-only` |
| FB-20260628-170315 | `8dd7a37db770d707f85dcccff8c74b9ff5edc7158ac8c0fc730dd5ea7b0ac56c` | `local-only` |
| FB-20260628-170330 | `ddbc8530a2cf53b0a7be1bec7ab3efff7a470cff78d55d4c8b01fa99b534b738` | `local-only` |
| FB-20260628-170359 | `ed2732f296b8776bb5e3ef67a5cdd4b0d8e4252bff1b85839da45e372fec4264` | `local-only` |
| FB-20260628-170424 | `a37e0f34b240720df6c1cf53f82720f55831e4059b6db6218c5a7eb09e244a27` | `local-only` |
| FB-20260628-170457 | `4f0b490a07b8c1434e2562f9e2ee3a8265adc6729bd8e9573d8d7fc5f3ad3127` | `local-only` |
| FB-20260628-170710 | `474ce34f7722da88f9ff2eaebf747daccfafe0855ccef219ccf631d1abe42e7f` | `local-only` |
| FB-20260628-170828 | `3368518b135ab4de6d37fcb3185b8d5b34c55ffeb41ba77448f42130c48237c1` | `local-only` |
| FB-20260628-170847 | `491ae848f8f168df9a06f05a81e37d74d872f58dfc2b87342cebfe5ce8665a0a` | `local-only` |
| FB-20260628-170850 | `90b4214b4503ea90c10d3ee229d06bd7800d5b19b0b3a456edf66ec702f56a4f` | `local-only` |
| FB-20260628-171219 | `67a4a48fa50732f3ebacfd975697f590f3484accaedf70a15c265602578ed5d3` | `local-only` |
| FB-20260628-171254 | `4c89fe9a4c4400d844d5a891b9f8384698c16352a75a419a4f89af5280c0e253` | `local-only` |
| FB-20260629-031259 | `1644351aa1d2681ad1c27cfc961232675932a31b41fda46e4ebc3a69bc9295e8` | `local-only` |
| FB-20260630-052233 | `6a10cd47437febbaec0cea94125500f0abb4bf3ffbce05a07afb2b767704102c` | `local-only` |
| FB-20260630-052353 | `a9a9b19d84c6eb6a4025e1c37ece14626e546e35cff17c75327666921eb9a8c1` | `local-only` |
| FB-20260630-052442 | `c5f7be5ac3b959e82eabd4e273f8bb1a4b3a365b99517dc052e25fb6503c4d73` | `local-only` |
| FB-20260630-052525 | `c4f96ba65b031e9a85186b8a48b4e792cced290d3dc958d05371c8fce9563416` | `local-only` |
| FB-20260630-052809 | `141f2b84da0bbae526094eafc2f87f7648f2e3061e40f4101516e7e36b724ad9` | `local-only` |
<!-- END GENERATED LOCAL EVIDENCE -->

## Closure Buckets

| Bucket | Cases | Closure evidence |
|--------|------:|------------------|
| Manufacture recommendation | 8 | `7b9b5d1`, `a900fb6`, `82a1001`, `1d16381`, `cee17b2`, related manufacture tests |
| Trade recommendation / station semantics | 4 | `74b3a84`, `e2d1496`, `8c037f7`, `c357377`, trade role tests |
| Control center / global injection | 5 | `7425b08`, `afff724`, control layered-fill tests |
| Power plant | 3 | `becbe08`, `9203299`, power assignment tests |
| Dormitory / layout metadata | 4 | `16bf4bd`, `af77f6a`, layout/MAA dorm semantics |
| Side facility fill | 2 | Current MAA/export behavior; kept in smoke matrix |
| Layout metadata / account analysis | 1 | `16bf4bd`, frontend/layout docs and metadata shape |

## Case Ledger

| ID | Status | Area | Source | Closure / coverage |
|----|--------|------|--------|--------------------|
| FB-20260627-085944 | closed | manufacture/gold | `local-evidence:2026-06-27/085944-褐果可以替换成夜烟30-效率` | Haze/Night Smoke modeling fixed by `a900fb6`; covered by manufacture search smoke. |
| FB-20260627-090047 | closed | layout/trade metadata | `local-evidence:2026-06-27/090047-252缺少等级分布` | Layout level semantics normalized by `16bf4bd`; keep in frontend/layout metadata smoke. |
| FB-20260628-131551 | closed | control/global inject | `local-evidence:2026-06-28/131551-中枢制造加2重复` | Covered by layered control scheduling (`7425b08`) and control inject split smoke. |
| FB-20260628-132114 | closed | dormitory filler | `local-evidence:2026-06-28/132114-宿舍会塞一些乱七八糟的干员` | Dorm/filler semantics corrected by `16bf4bd` and `af77f6a`; treat dorm fill as P2 smoke, not optimization scope. |
| FB-20260628-160142 | closed | trade/Vina | `local-evidence:2026-06-28/160142-维娜维多利亚在这里只有30-效率-不如任意的35-贸易效率-或者将隔壁伺夜跨站放过来` | Vina Glasgow peer bonus fixed by `74b3a84`; role priority covered by trade fill tests. |
| FB-20260628-160411 | closed | power/fallback capacity | `local-evidence:2026-06-28/160411-发电站候选不足-当前布局有-3-个发电站-但-infra-cli-只生成了-2-组发电站排班-cli-日志显示当前-box-筛选后只有-2-名可用于发电站的候选` | Power whitelist fallback capacity fixed by `becbe08`; covered by `power_filter_falls_back_when_whitelist_cannot_fill_all_rooms`. |
| FB-20260628-162229 | closed | manufacture/record | `local-evidence:2026-06-28/162229-更建议使用红云-红云酒神克里斯汀的组合会比当前组合高1` | Christine combo search/modeling fixed by `b0963dc` and `7b9b5d1`; covered by manufacture search smoke. |
| FB-20260628-162304 | closed | manufacture/Waai Fu pairing | `local-evidence:2026-06-28/162304-断罪者35-无法完整触发槐琥40-效率-推荐槐琥进入3级站-或者二级站搭配迷迭香等40-以上的角色` | Covered by `82a1001` Huaihu manufacture pairing test. |
| FB-20260628-170234 | duplicate-covered | trade/Vina | `local-evidence:2026-06-28/170234-维娜维多利亚在没有推王组的情况下只有30效率-降低优先级` | Duplicate family of FB-20260628-160142; fixed by `74b3a84`. |
| FB-20260628-170245 | duplicate-covered | manufacture/record | `local-evidence:2026-06-28/170245-红云酒神克里斯汀` | Duplicate family of FB-20260628-162229; covered by Christine combo fixes. |
| FB-20260628-170315 | closed | manufacture/gold + power link | `local-evidence:2026-06-28/170315-推荐调整成清流温蒂冬时-挂钩发电承曦格雷伊-130` | Trace seed added by `1d16381` and regression by `cee17b2`; covered by manufacture trace debug smoke. |
| FB-20260628-170330 | closed | power/linked producer | `local-evidence:2026-06-28/170330-替换成绑定森蚺的承曦格雷伊` | Current policy keeps pure power sorting and surfaces linked value through manufacture resolve/trace; covered by power search tests and manufacture trace seed. |
| FB-20260628-170359 | closed | control/Kal'tsit Mon3tr | `local-evidence:2026-06-28/170359-m3-凯尔希技能重复-m3严格大于凯尔希` | Covered by layered control fill policy and `control_layered_fill_prefers_efficiency_piece_over_mood`. |
| FB-20260628-170424 | closed | control/conditional team | `local-evidence:2026-06-28/170424-斩业星熊需要有陈-诗怀雅同在中枢-现在缺少陈` | Covered by layered control scheduling (`7425b08`) and control fill smoke. |
| FB-20260628-170457 | closed | trade/schedule station binding | `local-evidence:2026-06-28/170457-巫恋组不应变更贸易站-否则会重复暖机` | Witch group station binding fixed by `e2d1496` and warmup preservation by `8c037f7`. |
| FB-20260628-170710 | closed | power/cart priority | `local-evidence:2026-06-28/170710-小车类基础只有10-发电-优先级降低-最好是除非被唤起否则不用-凯尔希-f3的15-发电-仍然不如直接放一个常见的15-或者20-角色` | Ordinary chargers preferred over carts by `9203299`; covered by `power_filter_prefers_ordinary_chargers_before_friston` and `power_assignment_tie_prefers_plain_charger_over_friston_combo`. |
| FB-20260628-170828 | closed | dormitory/layout levels | `local-evidence:2026-06-28/170828-3发电-宿舍应该是5级满级` | Dorm level semantics normalized by `16bf4bd`; keep in layout metadata smoke. |
| FB-20260628-170847 | closed | meeting fill/export | `local-evidence:2026-06-28/170847-空置了-未放人` | Current export/fill policy no longer tracks this as open; include side-facility nonempty/export shape in smoke if regressions reappear. |
| FB-20260628-170850 | closed | office fill/export | `local-evidence:2026-06-28/170850-空置了-未放人` | Same closure family as FB-20260628-170847; include office/meeting export shape in smoke. |
| FB-20260628-171219 | closed | dormitory/layout levels | `local-evidence:2026-06-28/171219-252-32贸易-33222制造的四间宿舍-等级应当是2级-1级-1级-1级` | Dorm layout semantics normalized by `16bf4bd`; keep 252 layout metadata in smoke. |
| FB-20260628-171254 | closed | dormitory/synergy guard | `local-evidence:2026-06-28/171254-乌尔比安此时完全无用-没有用深巡或者深海-宿舍不用塞乌尔比安` | Unused Ulpian dorm anchor fixed by `af77f6a`. |
| FB-20260629-031259 | closed | trade/filler exclusion | `local-evidence:2026-06-29/031259-载入全精二样例-不应该会出现尤里卡-u-official` | Eureka/U-Official excluded from trade scheduling by `c357377`. |
| FB-20260630-052233 | closed | manufacture/schedule duration | `local-evidence:2026-06-30/052233-铅踝工作时间过长` | Re-reported by `local-evidence:2026-07-28/194234-铅踝避免上12h班次-平均效率会逐步降低`; closed with capability-based `>=12h` continuous-work admission, net-mood shift averaging, ordinary/anchor/capacity regressions, and current-runtime bundle replay. |
| FB-20260630-052353 | closed | manufacture/system completeness | `local-evidence:2026-06-30/052353-那斯提挂件没满` | Covered by current manufacture/system smoke; no separate open item remains. |
| FB-20260630-052442 | closed | manufacture/gold | `local-evidence:2026-06-30/052442-蛇屠箱可以换成豆苗` | Covered by current manufacture search smoke; no separate open item remains. |
| FB-20260630-052525 | duplicate-covered | control/same-effect conflict | `local-evidence:2026-06-30/052525-望和诗怀雅技能冲突-都是-7` | Same control stacking family as FB-20260628-131551; covered by control inject/fill smoke. |
| FB-20260630-052809 | duplicate-covered | control/filler opportunity cost | `local-evidence:2026-06-30/052809-mon3tr换成其他的` | Same control filler family as FB-20260628-170359; covered by layered control fill tests. |

## Smoke Matrix

Run this matrix after touching layout assignment, schedule, search, control, power, manufacture, or frontend/serve output.

```bash
mkdir -p target/codex-logs

cargo test -p infra-core --no-run > target/codex-logs/infra-core-test-build.log 2>&1
tail -80 target/codex-logs/infra-core-test-build.log
cargo test -p infra-core --quiet

cargo build -p infra-cli > target/codex-logs/infra-cli-build.log 2>&1
tail -80 target/codex-logs/infra-cli-build.log
cargo run -q -p infra-cli -- verify --all

cargo run -q -p infra-cli -- plan \
  --operbox data/fixtures/243/operbox_full_e2.json \
  --maa-out out/243_maa.json
```

Targeted tests worth naming in PR notes:

| Feedback family | Existing targeted coverage |
|-----------------|----------------------------|
| Power candidate fallback | `power_filter_falls_back_when_whitelist_cannot_fill_all_rooms` |
| Weak cart priority | `power_filter_prefers_ordinary_chargers_before_friston`, `power_assignment_tie_prefers_plain_charger_over_friston_combo` |
| Control filler quality | `control_plugin_fill_excludes_unclaimed_system_producers`, `control_layered_fill_prefers_efficiency_piece_over_mood` |
| Purestream/Weedy/Windflit trace | `manufacture_trace_sink_reports_operator_used_for_later_gold_room`, `data/feedback_regression_seeds/purestream_weedy_windflit_gold_automation.json` |
| Waai Fu pairing | Huaihu manufacture pairing test added by `82a1001` |
| Leadfoot long manufacture shift | `leadfoot_shift_average_uses_actual_net_mood_drain`, `long_shift_excludes_mood_gap_decay_but_short_shift_keeps_it`, `mood_gap_decay_validator_rejects_adjacent_short_shifts` |
| Vina / witch / U-Official trade regressions | Trade role and schedule binding tests around `meta_vina`, `witch`, and Eureka exclusion |

## Reopen Rule

If a user reports one of these symptoms again, do not create a new architecture plan. Reopen the matching `FB-*` row, paste the new reproduction command, and localize the regression to the smallest layer before changing code.

## 2026-08-06 batch (synced from 114.66.55.78)

| ID | Status | Area | Source | Closure / coverage |
|----|--------|------|--------|--------------------|
| FB-20260730-004002 | closed | meeting fill | `feedback/2026-07-30/004002-会客室输出不稳定-时有时无` | Solver now fills the meeting room (two seats, clue-speed skilled first, idle fallback) in the single-shift pipeline and ABC rotation cohorts; covered by `assign_meeting_rooms` tests and `abc_rotation_fills_meeting_room_with_rotating_meeting_skilled_cohort`. |
| FB-20260802-145054 | upstream-frontend | manufacture roster | `feedback/2026-08-02/145054-没练红松林-检查下森空岛识别哪里出问题了` | The reported run used the "243 全精二示例" operbox (418 e2-90), not the user's own box; solver behavior was correct. Frontend data-source issue; needs the user's synced operbox to chase 森空岛 recognition. |
| FB-20260802-145242 | upstream-frontend | trade roster | `feedback/2026-08-02/145242-没维娜-我402干员扫出来404个-森空岛上看也只有402个-看看哪里出问题了-另外-加油鸭-这东西要整出来将轰动舟区-祝好运` | Same run `b26e6f08` used the sample operbox; user's real export (07:53) owns 390 and lacks 维娜·维多利亚/远牙. Solver faithful; 森空岛 402→404 recognition needs frontend-side repro. |

Upstream-frontend rows are evidence of frontend/skland recognition issues, not core-solver defects; reopen only with the user's actual operbox attached.
