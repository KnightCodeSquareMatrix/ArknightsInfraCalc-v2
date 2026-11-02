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
            .map(|id| {
                self.header
                    .operator_names
                    .get(*id)
                    .cloned()
                    .ok_or_else(|| Error::msg("Bake state ID out of range"))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut row = BakedComboRow {
            facility: row_facility_from_signature(&self.header.indexes[signature].signature_key)
                .to_string(),
            room_level: meta.level,
            operator_capacity: meta.capacity,
            names,
            operator_mask: operator_mask_from_indices(
                &ids,
                self.header.operator_names.len().div_ceil(64).max(1),
            ),
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
                row.sort_efficiency = Efficiency::from_millis(data.efficiencies[7]);
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
                row.sort_efficiency = Efficiency::from_millis(data.efficiencies[4]);
            }
        }
        Ok(row)
    }
}

pub(super) fn read(
    header: BakedComboTableDisk,
    reader: &mut std::io::BufReader<File>,
) -> Result<Table> {
    if header.operator_count > usize::from(u16::MAX) + 1 {
        return Err(Error::msg("Bake compact state IDs exceed u16"));
    }
    let mut result = Table {
        header,
        signatures: Vec::new(),
        rules: Vec::new(),
        signature_by_slot: std::array::from_fn(|_| None),
    };
    let mut rule_ids = HashMap::new();
    for index in &result.header.indexes {
        if index.len > u32::MAX as usize {
            return Err(Error::msg("Bake row ID exceeds u32"));
        }
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
        result.signature_by_slot[slot] = Some(result.signatures.len());
        match &mut signature.rows {
            Rows::Trade(rows) => rows.try_reserve_exact(index.len),
            Rows::Manufacture(rows) => rows.try_reserve_exact(index.len),
        }
        .map_err(|error| Error::msg(format!("cannot allocate compact Bake rows: {error}")))?;
        let mut pending = Some(first);
        for _ in 0..index.len {
            let row = if let Some(row) = pending.take() {
                row
            } else {
                stream::decode_from(&mut *reader)?
            };
            let mut indices = [0u16; 3];
            for (slot, id) in row.operator_indices.iter().enumerate() {
                *indices
                    .get_mut(slot)
                    .ok_or_else(|| Error::msg("cannot decode Bake row into three state slots"))? =
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
                            row.trade_base_efficiency_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_occupancy_efficiency_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_skill_efficiency_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_control_efficiency_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_paper_efficiency_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_mechanic_equivalent_efficiency_millis
                                .ok_or_else(|| {
                                    Error::msg("cannot decode missing Bake response value")
                                })?,
                            row.trade_unit_output_multiplier_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_final_efficiency_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_equivalent_skill_efficiency_millis
                                .ok_or_else(|| {
                                    Error::msg("cannot decode missing Bake response value")
                                })?,
                            row.trade_gold_equivalent_efficiency_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_combined_output_value_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.trade_total_efficiency_millis.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                        ],
                        units: [
                            row.unit_trade_per_day.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.unit_gold_per_day.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.unit_gold_consumption_per_day.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                            row.unit_originium_per_day.ok_or_else(|| {
                                Error::msg("cannot decode missing Bake response value")
                            })?,
                        ],
                        rule,
                    });
                }
                Rows::Manufacture(rows) => rows.push(ManuRow {
                    indices,
                    efficiencies: [
                        row.manufacture_base_efficiency_millis.ok_or_else(|| {
                            Error::msg("cannot decode missing Bake response value")
                        })?,
                        row.manufacture_occupancy_efficiency_millis.ok_or_else(|| {
                            Error::msg("cannot decode missing Bake response value")
                        })?,
                        row.manufacture_skill_efficiency_millis.ok_or_else(|| {
                            Error::msg("cannot decode missing Bake response value")
                        })?,
                        row.manufacture_global_efficiency_millis.ok_or_else(|| {
                            Error::msg("cannot decode missing Bake response value")
                        })?,
                        row.manufacture_final_efficiency_millis.ok_or_else(|| {
                            Error::msg("cannot decode missing Bake response value")
                        })?,
                    ],
                    storage: row
                        .manufacture_storage_limit
                        .ok_or_else(|| Error::msg("cannot decode missing Bake response value"))?,
                    outputs: [
                        row.manufacture_unit_output_per_day.ok_or_else(|| {
                            Error::msg("cannot decode missing Bake response value")
                        })?,
                        row.manufacture_output_per_day.ok_or_else(|| {
                            Error::msg("cannot decode missing Bake response value")
                        })?,
                    ],
                }),
            }
        }
        result.signatures.push(signature);
    }
    Ok(result)
}
