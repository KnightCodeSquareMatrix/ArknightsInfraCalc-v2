//! Complete room candidates and exact joint selection under a supplied shared state.
//! Plan owns admission/room bindings; the caller owns construction of shared states.

use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;
use smallvec::SmallVec;

use crate::error::{Error, Result};
use crate::layout::SharedLayout;
use crate::pool::TradePool;
use crate::trade::input::TradeOrderKind;
use crate::{Efficiency, SkillTable};

use super::TradeSearchOptions;

#[derive(Clone)]
pub(crate) struct TradeAllocationRoom {
    pub options: TradeSearchOptions,
    pub order: TradeOrderKind,
    pub allowed: Vec<bool>,
    pub required: Vec<usize>,
}

pub(crate) struct TradeAllocationState {
    pub layout: SharedLayout,
    pub allowed: Vec<bool>,
    pub required: Vec<usize>,
}

pub(crate) struct TradeAllocationRequest {
    pub rooms: Vec<TradeAllocationRoom>,
    pub states: Vec<TradeAllocationState>,
    pub varying: Vec<bool>,
    pub forbidden_pairs: Vec<(usize, usize)>,
}

/// Domain objective: gold first, originium only breaks equal gold scores.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TradeAllocationScore {
    pub gold: Efficiency,
    pub originium: Efficiency,
}

impl TradeAllocationScore {
    pub fn add_room(mut self, order: TradeOrderKind, efficiency: Efficiency) -> Self {
        if order.is_gold() {
            self.gold += efficiency;
        } else {
            self.originium += efficiency;
        }
        self
    }

    fn plus(self, other: Self) -> Self {
        Self {
            gold: self.gold + other.gold,
            originium: self.originium + other.originium,
        }
    }
}

pub(crate) struct TradeAllocationResult {
    pub state: usize,
    pub rooms: Vec<SmallVec<[usize; 3]>>,
    pub score: TradeAllocationScore,
}

#[derive(Clone)]
struct Candidate {
    members: SmallVec<[usize; 3]>,
    score: Efficiency,
}

fn room_combinations(
    room: &TradeAllocationRoom,
    forbidden: &[(usize, usize)],
) -> Vec<SmallVec<[usize; 3]>> {
    fn visit(
        available: &[usize],
        next: usize,
        room: &TradeAllocationRoom,
        forbidden: &[(usize, usize)],
        selected: &mut SmallVec<[usize; 3]>,
        out: &mut Vec<SmallVec<[usize; 3]>>,
    ) {
        if room.required.iter().all(|index| selected.contains(index)) {
            out.push(selected.clone());
        }
        if selected.len() == room.options.operator_capacity {
            return;
        }
        for offset in next..available.len() {
            let index = available[offset];
            if forbidden.iter().any(|&(a, b)| {
                (index == a && selected.contains(&b)) || (index == b && selected.contains(&a))
            }) {
                continue;
            }
            selected.push(index);
            visit(available, offset + 1, room, forbidden, selected, out);
            selected.pop();
        }
    }
    let available: Vec<_> = room
        .allowed
        .iter()
        .enumerate()
        .filter_map(|(index, allowed)| allowed.then_some(index))
        .collect();
    let mut out = Vec::new();
    visit(
        &available,
        0,
        room,
        forbidden,
        &mut SmallVec::new(),
        &mut out,
    );
    out
}

struct JointSearch<'a> {
    request: &'a TradeAllocationRequest,
    lists: &'a [Vec<Candidate>],
    required: &'a [usize],
    suffix: Vec<TradeAllocationScore>,
    used: Vec<bool>,
    selected: Vec<SmallVec<[usize; 3]>>,
    best: &'a mut Option<TradeAllocationResult>,
    state: usize,
}

impl JointSearch<'_> {
    fn visit(&mut self, room_index: usize, score: TradeAllocationScore) {
        if self
            .best
            .as_ref()
            .is_some_and(|best| score.plus(self.suffix[room_index]) <= best.score)
        {
            return;
        }
        if room_index == self.lists.len() {
            if self.required.iter().all(|&index| self.used[index]) {
                *self.best = Some(TradeAllocationResult {
                    state: self.state,
                    rooms: self.selected.clone(),
                    score,
                });
            }
            return;
        }
        for candidate in &self.lists[room_index] {
            let next = score.add_room(self.request.rooms[room_index].order, candidate.score);
            if self
                .best
                .as_ref()
                .is_some_and(|best| next.plus(self.suffix[room_index + 1]) <= best.score)
            {
                break;
            }
            if candidate.members.iter().any(|&index| self.used[index]) {
                continue;
            }
            for &index in &candidate.members {
                self.used[index] = true;
            }
            self.selected.push(candidate.members.clone());
            self.visit(room_index + 1, next);
            self.selected.pop();
            for &index in &candidate.members {
                self.used[index] = false;
            }
        }
    }
}

pub(crate) fn search_trade_allocation(
    pool: &TradePool,
    table: &SkillTable,
    request: &TradeAllocationRequest,
) -> Result<TradeAllocationResult> {
    let mut stage = crate::profile::start_solver_stage("search", "trade_allocation");
    let mut perf = crate::profile::start_search_perf(crate::profile::SearchOperation::Trade);
    let prepare_started = Instant::now();
    let combinations: Vec<_> = request
        .rooms
        .iter()
        .map(|room| room_combinations(room, &request.forbidden_pairs))
        .collect();
    // None caches a rejected combination as well. Stable scores are evaluated once,
    // without top-K truncation; every state still filters its actual membership.
    let mut stable: Vec<Vec<Option<Efficiency>>> = combinations
        .iter()
        .map(|items| vec![None; items.len()])
        .collect();
    let prepare = prepare_started.elapsed();
    perf.mark_live_started();
    let counters = perf.hot_path_batch_counters();
    let mut best = None;
    let mut attempts = 0;
    let mut evaluated = 0;
    let mut evaluate = Default::default();
    let mut sort_topk = Default::default();
    for (state_index, state) in request.states.iter().enumerate() {
        let mut lists = Vec::new();
        for (room_index, room) in request.rooms.iter().enumerate() {
            let mut options = room.options.clone();
            options.layout = Arc::clone(&state.layout);
            let started = Instant::now();
            let scores: Vec<_> = combinations[room_index]
                .par_iter()
                .enumerate()
                .map_init(
                    || crate::profile::start_hot_path_batch(Arc::clone(&counters)),
                    |_, (index, combo)| {
                        if combo.iter().any(|&member| !state.allowed[member]) {
                            return (None, false);
                        }
                        let varies = combo.iter().any(|&member| request.varying[member]);
                        if !varies && state_index > 0 {
                            return (stable[room_index][index], false);
                        }
                        let score = if combo.is_empty() {
                            Some(Efficiency::ZERO)
                        } else {
                            options
                                .solve_trade_combo(pool, table, room.order, combo, None, None)
                                .map(|(hit, _)| hit.final_efficiency)
                        };
                        (score, !combo.is_empty())
                    },
                )
                .collect();
            evaluate += started.elapsed();
            let started = Instant::now();
            let mut list = Vec::new();
            for (index, (score, solved)) in scores.into_iter().enumerate() {
                attempts += u64::from(solved);
                evaluated += u64::from(solved && score.is_some());
                if state_index == 0
                    && !combinations[room_index][index]
                        .iter()
                        .any(|&member| request.varying[member])
                {
                    stable[room_index][index] = score;
                }
                if let Some(score) = score {
                    list.push(Candidate {
                        members: combinations[room_index][index].clone(),
                        score,
                    });
                }
            }
            list.sort_by(|a, b| b.score.cmp(&a.score).then(a.members.cmp(&b.members)));
            sort_topk += started.elapsed();
            lists.push(list);
        }
        if lists.iter().any(Vec::is_empty) {
            continue;
        }
        let mut suffix = vec![TradeAllocationScore::default(); lists.len() + 1];
        for index in (0..lists.len()).rev() {
            suffix[index] =
                suffix[index + 1].add_room(request.rooms[index].order, lists[index][0].score);
        }
        JointSearch {
            request,
            lists: &lists,
            required: &state.required,
            suffix,
            used: vec![false; pool.entries.len()],
            selected: Vec::new(),
            best: &mut best,
            state: state_index,
        }
        .visit(0, TradeAllocationScore::default());
    }
    let generated = combinations.iter().map(|items| items.len() as u64).sum();
    perf.finish(
        generated,
        attempts,
        evaluated,
        u64::from(best.is_some()),
        false,
        crate::profile::SearchPhaseDurations {
            prepare,
            evaluate,
            sort_topk,
            ..Default::default()
        },
    );
    let result =
        best.ok_or_else(|| Error::no_candidate("no joint trade allocation under Plan bindings"));
    stage.record_result(&result);
    result
}
