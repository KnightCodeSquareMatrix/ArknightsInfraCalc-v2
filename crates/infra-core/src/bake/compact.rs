//! Runtime rows keep exact numeric values; metadata and rules are shared.
use super::*;

#[derive(Debug)]
pub(super) struct TradeRow {
    pub indices: [u16; 3],
    pub efficiencies: [i32; 12],
    pub units: [f64; 4],
    rule: u32,
}

#[derive(Debug)]
pub(super) struct ManuRow {
    pub indices: [u16; 3],
    pub efficiencies: [i32; 5],
    pub storage: i32,
    pub outputs: [f64; 2],
}

#[derive(Debug)]
pub(super) enum Rows {
    Trade(Vec<TradeRow>),
    Manufacture(Vec<ManuRow>),
}

#[derive(Debug)]
pub(super) struct Signature {
    pub level: u8,
    pub capacity: usize,
    pub order: Option<TradeOrderKind>,
    pub recipe: Option<RecipeKind>,
    pub rows: Rows,
}

#[derive(Debug)]
pub(super) struct Table {
    pub header: BakedComboTableDisk,
    pub signatures: Vec<Signature>,
    pub byte_offsets: Vec<u64>,
    pub rules: Vec<String>,
    pub signature_by_slot: [Option<usize>; BAKED_SIGNATURE_SLOTS],
}

impl Signature {
    pub fn indices(&self, offset: usize) -> &[u16] {
        match &self.rows {
            Rows::Trade(rows) => &rows[offset].indices[..self.capacity],
            Rows::Manufacture(rows) => &rows[offset].indices[..self.capacity],
        }
    }

    pub fn len(&self) -> usize {
        match &self.rows {
            Rows::Trade(rows) => rows.len(),
            Rows::Manufacture(rows) => rows.len(),
        }
    }

    pub fn slot(&self) -> Result<usize> {
        match (self.order, self.recipe) {
            (Some(order), None) => trade_signature_slot(self.level, self.capacity, order),
            (None, Some(recipe)) => manufacture_signature_slot(self.level, self.capacity, recipe),
            _ => None,
        }
        .ok_or_else(|| Error::msg("unsupported Bake signature"))
    }
}

impl Table {
    /// Only selected/output rows acquire owned names and optional fields.
    pub fn materialize(&self, signature: usize, offset: usize) -> Result<BakedComboRow> {
        let meta = &self.signatures[signature];
        if offset >= meta.len() {
            return Err(Error::msg("Bake row ID out of range"));
        }
        let ids = meta
            .indices(offset)
            .iter()
            .map(|id| usize::from(*id))
            .collect::<Vec<_>>();
        let names = ids
            .iter()
            .map(|id| self.header.operator_names[*id].clone())
            .collect();
        let mut row = BakedComboRow {
            facility: row_facility_from_signature(&self.header.indexes[signature].signature_key)
                .to_string(),
            room_level: meta.level,
            operator_capacity: meta.capacity,
            names,
            operator_mask: operator_mask_from_indices(&ids, self.header.mask_words),
            operator_indices: ids,
            sort_efficiency: Efficiency::ZERO,
            order_kind: meta.order,
            recipe: meta.recipe,
            trade_mechanism_signature: None,
            trade_base_efficiency: None,
            trade_occupancy_efficiency: None,
            trade_skill_efficiency: None,
            trade_control_efficiency: None,
            trade_paper_efficiency: None,
            trade_mechanic_equivalent_efficiency: None,
            trade_unit_output_multiplier: None,
            trade_final_efficiency: None,
            trade_equivalent_skill_efficiency: None,
            trade_gold_equivalent_efficiency: None,
            trade_combined_output_value: None,
            trade_total_efficiency: None,
            rule_id: None,
            unit_trade_per_day: None,
            unit_gold_per_day: None,
            unit_gold_consumption_per_day: None,
            unit_originium_per_day: None,
            manufacture_base_efficiency: None,
            manufacture_occupancy_efficiency: None,
            manufacture_skill_efficiency: None,
            manufacture_global_efficiency: None,
            manufacture_final_efficiency: None,
            manufacture_storage_limit: None,
            manufacture_unit_output_per_day: None,
            manufacture_output_per_day: None,
        };
        match &meta.rows {
            Rows::Trade(rows) => {
                let data = &rows[offset];
                let e = data.efficiencies.map(|e| Some(Efficiency::from_millis(e)));
                [
                    row.trade_base_efficiency,
                    row.trade_occupancy_efficiency,
                    row.trade_skill_efficiency,
                    row.trade_control_efficiency,
                    row.trade_paper_efficiency,
                    row.trade_mechanic_equivalent_efficiency,
                    row.trade_unit_output_multiplier,
                    row.trade_final_efficiency,
                    row.trade_equivalent_skill_efficiency,
                    row.trade_gold_equivalent_efficiency,
                    row.trade_combined_output_value,
                    row.trade_total_efficiency,
                ] = e;
                [
                    row.unit_trade_per_day,
                    row.unit_gold_per_day,
                    row.unit_gold_consumption_per_day,
                    row.unit_originium_per_day,
                ] = data.units.map(Some);
                row.sort_efficiency = row.trade_final_efficiency.unwrap();
                if data.rule != u32::MAX {
                    row.rule_id = Some(self.rules[data.rule as usize].clone());
                }
            }
            Rows::Manufacture(rows) => {
                let data = &rows[offset];
                [
                    row.manufacture_base_efficiency,
                    row.manufacture_occupancy_efficiency,
                    row.manufacture_skill_efficiency,
                    row.manufacture_global_efficiency,
                    row.manufacture_final_efficiency,
                ] = data.efficiencies.map(|e| Some(Efficiency::from_millis(e)));
                row.manufacture_storage_limit = Some(data.storage);
                [
                    row.manufacture_unit_output_per_day,
                    row.manufacture_output_per_day,
                ] = data.outputs.map(Some);
                row.sort_efficiency = row.manufacture_final_efficiency.unwrap();
            }
        }
        Ok(row)
    }
}

pub(super) fn read_validated(
    header: BakedComboTableDisk,
    reader: &mut std::io::BufReader<File>,
    trade_pool: &TradePool,
    manu_pool: &ManuPool,
    skill_table: &SkillTable,
) -> Result<Table> {
    use std::io::Seek;
    if header.operator_count > usize::from(u16::MAX) + 1 {
        return Err(Error::msg("Bake compact state IDs exceed u16"));
    }
    let operator_index = header
        .operator_states
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, state)| (state, id))
        .collect::<HashMap<_, _>>();
    let trade_canonical =
        canonical_state_ranks(trade_pool, OperatorState::from_trade_entry, &operator_index)?;
    let manu_canonical =
        canonical_state_ranks(manu_pool, OperatorState::from_manu_entry, &operator_index)?;
    let mut result = Table {
        header,
        signatures: Vec::new(),
        byte_offsets: Vec::new(),
        rules: Vec::new(),
        signature_by_slot: std::array::from_fn(|_| None),
    };
    let mut rule_ids = HashMap::new();
    for index in &result.header.indexes {
        if index.len > u32::MAX as usize {
            return Err(Error::msg("Bake row ID exceeds u32"));
        }
        result.byte_offsets.push(reader.stream_position()?);
        let first: BakedComboRowDisk = stream::decode_from(&mut *reader)?;
        let mut signature = Signature {
            level: first.room_level,
            capacity: first.operator_capacity,
            order: first.order_kind,
            recipe: first.recipe,
            rows: if first.order_kind.is_some() {
                Rows::Trade(Vec::new())
            } else {
                Rows::Manufacture(Vec::new())
            },
        };
        let slot = signature.slot()?;
        if result.signature_by_slot[slot]
            .replace(result.signatures.len())
            .is_some()
        {
            return Err(Error::msg("duplicate Bake signature slot"));
        }
        match &mut signature.rows {
            Rows::Trade(rows) => rows.try_reserve_exact(index.len),
            Rows::Manufacture(rows) => rows.try_reserve_exact(index.len),
        }
        .map_err(|error| Error::msg(format!("cannot allocate compact Bake rows: {error}")))?;
        // One bit per possible combo, including illegal ones. Bits are cleared
        // by enumerating the original legal universe after reading this signature.
        let ranks = rank::choose(result.header.operator_count, signature.capacity)?;
        let mut actual = Vec::new();
        actual
            .try_reserve_exact(ranks.div_ceil(64))
            .map_err(|error| {
                Error::msg(format!("cannot allocate Bake universe bitmap: {error}"))
            })?;
        actual.resize(ranks.div_ceil(64), 0u64);
        let mut previous: Option<(i32, i32, Vec<usize>)> = None;
        let mut pending = Some(first);
        for _ in 0..index.len {
            let row = if let Some(row) = pending.take() {
                row
            } else {
                stream::decode_from(&mut *reader)?
            };
            validate_baked_row_for_signature(&row, &index.signature_key)?;
            if row.operator_capacity != row.operator_indices.len()
                || row
                    .operator_indices
                    .iter()
                    .any(|id| *id >= result.header.operator_count)
                || !distinct_names(
                    row.operator_indices
                        .iter()
                        .map(|id| result.header.operator_names[*id].as_str()),
                )
            {
                return Err(Error::msg(format!(
                    "invalid baked row in {:?}",
                    index.signature_key
                )));
            }
            let position = rank::colex(&row.operator_indices)?;
            if position >= ranks {
                return Err(Error::msg("Bake row rank out of range"));
            }
            let bit = 1u64 << (position % 64);
            if actual[position / 64] & bit != 0 {
                return Err(Error::msg("duplicate Bake combo rank"));
            }
            actual[position / 64] |= bit;
            let storage = row.manufacture_storage_limit.unwrap_or_default();
            if previous.as_ref().is_some_and(|(eff, old_storage, ids)| {
                *eff < row.sort_efficiency_millis
                    || (*eff == row.sort_efficiency_millis
                        && (*old_storage < storage
                            || (*old_storage == storage && *ids > row.operator_indices)))
            }) {
                return Err(Error::msg(format!(
                    "baked rows are not sorted for {:?}",
                    index.signature_key
                )));
            }
            previous = Some((
                row.sort_efficiency_millis,
                storage,
                row.operator_indices.clone(),
            ));
            let canonical = if signature.order.is_some() {
                &trade_canonical
            } else {
                &manu_canonical
            };
            let mut combo = row
                .operator_indices
                .iter()
                .map(|id| {
                    canonical.get(id).copied().ok_or_else(|| {
                        Error::msg("baked state missing from canonical facility pool")
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            combo.sort_unstable();
            let expected = if signature.order.is_some() {
                let operators = build_trade_combo_operators_vec(trade_pool, &combo, None, None);
                if trade_station_exclusive_violation(&operators, skill_table) {
                    return Err(Error::msg("illegal baked trade combo"));
                }
                if trade_room_mechanism_signature(&operators) != row.trade_mechanism_signature {
                    return Err(Error::msg("baked trade mechanism signature mismatch"));
                }
                state_indices(operators.iter().map(trade_operator_state), &operator_index)?
            } else {
                let operators = combo
                    .iter()
                    .map(|id| manu_pool.entries[*id].to_manu_operator())
                    .collect::<Vec<_>>();
                if !crate::search::manufacture_combo_allowed_for_shift(
                    &operators,
                    skill_table,
                    None,
                ) {
                    return Err(Error::msg("illegal baked manufacture combo"));
                }
                state_indices(
                    combo
                        .iter()
                        .map(|id| OperatorState::from_manu_entry(&manu_pool.entries[*id])),
                    &operator_index,
                )?
            };
            if expected != row.operator_indices {
                return Err(Error::msg(
                    "baked combo execution order differs from canonical construction",
                ));
            }
            let mut indices = [0u16; 3];
            for (slot, id) in row.operator_indices.iter().enumerate() {
                indices[slot] =
                    u16::try_from(*id).map_err(|_| Error::msg("Bake state ID exceeds u16"))?;
            }
            match &mut signature.rows {
                Rows::Trade(rows) => {
                    let rule = if let Some(rule) = &row.rule_id {
                        if let Some(id) = rule_ids.get(rule) {
                            *id
                        } else {
                            let id = u32::try_from(result.rules.len())
                                .ok()
                                .filter(|id| *id != u32::MAX)
                                .ok_or_else(|| Error::msg("Bake rule dictionary exceeds u32"))?;
                            result.rules.push(rule.clone());
                            rule_ids.insert(rule.clone(), id);
                            id
                        }
                    } else {
                        u32::MAX
                    };
                    rows.push(TradeRow {
                        indices,
                        efficiencies: [
                            row.trade_base_efficiency_millis.unwrap(),
                            row.trade_occupancy_efficiency_millis.unwrap(),
                            row.trade_skill_efficiency_millis.unwrap(),
                            row.trade_control_efficiency_millis.unwrap(),
                            row.trade_paper_efficiency_millis.unwrap(),
                            row.trade_mechanic_equivalent_efficiency_millis.unwrap(),
                            row.trade_unit_output_multiplier_millis.unwrap(),
                            row.trade_final_efficiency_millis.unwrap(),
                            row.trade_equivalent_skill_efficiency_millis.unwrap(),
                            row.trade_gold_equivalent_efficiency_millis.unwrap(),
                            row.trade_combined_output_value_millis.unwrap(),
                            row.trade_total_efficiency_millis.unwrap(),
                        ],
                        units: [
                            row.unit_trade_per_day.unwrap(),
                            row.unit_gold_per_day.unwrap(),
                            row.unit_gold_consumption_per_day.unwrap(),
                            row.unit_originium_per_day.unwrap(),
                        ],
                        rule,
                    });
                }
                Rows::Manufacture(rows) => rows.push(ManuRow {
                    indices,
                    efficiencies: [
                        row.manufacture_base_efficiency_millis.unwrap(),
                        row.manufacture_occupancy_efficiency_millis.unwrap(),
                        row.manufacture_skill_efficiency_millis.unwrap(),
                        row.manufacture_global_efficiency_millis.unwrap(),
                        row.manufacture_final_efficiency_millis.unwrap(),
                    ],
                    storage: row.manufacture_storage_limit.unwrap(),
                    outputs: [
                        row.manufacture_unit_output_per_day.unwrap(),
                        row.manufacture_output_per_day.unwrap(),
                    ],
                }),
            }
        }
        let n = if signature.order.is_some() {
            trade_pool.entries.len()
        } else {
            manu_pool.entries.len()
        };
        let mut legal_count = 0usize;
        for combo in rank::combinations(n, signature.capacity) {
            let combo = &combo[..signature.capacity];
            let ids = if signature.order.is_some() {
                if !distinct_names(combo.iter().map(|id| trade_pool.entries[*id].name.as_str())) {
                    continue;
                }
                let operators = build_trade_combo_operators_vec(trade_pool, combo, None, None);
                if trade_station_exclusive_violation(&operators, skill_table) {
                    continue;
                }
                state_indices(operators.iter().map(trade_operator_state), &operator_index)?
            } else {
                if !distinct_names(combo.iter().map(|id| manu_pool.entries[*id].name.as_str())) {
                    continue;
                }
                let operators = combo
                    .iter()
                    .map(|id| manu_pool.entries[*id].to_manu_operator())
                    .collect::<Vec<_>>();
                if !crate::search::manufacture_combo_allowed_for_shift(
                    &operators,
                    skill_table,
                    None,
                ) {
                    continue;
                }
                state_indices(
                    combo
                        .iter()
                        .map(|id| OperatorState::from_manu_entry(&manu_pool.entries[*id])),
                    &operator_index,
                )?
            };
            let position = rank::colex(&ids)?;
            let bit = 1u64 << (position % 64);
            if actual[position / 64] & bit == 0 {
                return Err(Error::msg(format!(
                    "baked row universe is incomplete for {:?}",
                    index.signature_key
                )));
            }
            actual[position / 64] &= !bit;
            legal_count += 1;
        }
        if legal_count != index.len || actual.iter().any(|word| *word != 0) {
            return Err(Error::msg(format!(
                "baked row universe is incomplete for {:?}",
                index.signature_key
            )));
        }
        result.signatures.push(signature);
    }
    Ok(result)
}
