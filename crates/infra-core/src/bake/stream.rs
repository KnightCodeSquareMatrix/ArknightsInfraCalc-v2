//! Bounded sorted runs and external merge preserve the disk row order.

use std::collections::{BTreeSet, BinaryHeap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bincode::Options;
use serde::{Deserialize, Serialize};

use super::{
    BakeComboTableSummary, BakeGeneratorFingerprint, BakedComboIndex, BakedComboRow,
    BakedComboRowDisk, BakedComboTableDisk, OperatorState, TradeRoomMechanismSignature,
    BAKE_SCHEMA_VERSION,
};
use crate::error::{Error, Result};

pub(super) struct SignatureRowBlocks {
    out_dir: PathBuf,
    blocks: Vec<SignatureRowBlock>,
}

struct SignatureRowBlock {
    facility: &'static str,
    signature_key: String,
    row_count: usize,
    path: Option<PathBuf>,
}

impl Drop for SignatureRowBlock {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            // Only paths successfully created by this block are owned here.
            let _ = fs::remove_file(path);
        }
    }
}

impl SignatureRowBlocks {
    pub(super) fn new(out_dir: &Path) -> Self {
        Self {
            out_dir: out_dir.to_path_buf(),
            blocks: Vec::new(),
        }
    }

    pub(super) fn row_count(&self) -> usize {
        self.blocks.iter().map(|block| block.row_count).sum()
    }

    pub(super) fn push_chunks(
        &mut self,
        facility: &'static str,
        signature_key: String,
        chunks: impl Iterator<Item = Vec<BakedComboRow>>,
    ) -> Result<RowSummary> {
        let mut summary = RowSummary::default();
        let mut levels: Vec<Vec<Run>> = Vec::new();
        for mut rows in chunks {
            if rows.is_empty() {
                continue;
            }
            super::sort_signature_rows(&mut rows);
            let (path, file) = create_row_block(&self.out_dir)?;
            let run = Run {
                path,
                rows: rows.len(),
            };
            let mut writer = BufWriter::with_capacity(IO_BUFFER, file);
            for row in rows {
                summary.row_count += 1;
                if let Some(signature) = &row.trade_mechanism_signature {
                    summary.mechanism_rows += 1;
                    summary.mechanism_signatures.insert(signature.clone());
                }
                encode_into(&mut writer, &BakedComboRowDisk::from(&row))?;
            }
            writer.flush()?;
            drop(writer);
            let mut carry = run;
            let mut level = 0;
            loop {
                if levels.len() == level {
                    levels.push(Vec::new());
                }
                levels[level].push(carry);
                if levels[level].len() < MERGE_FAN_IN {
                    break;
                }
                carry = merge_runs(&self.out_dir, std::mem::take(&mut levels[level]))?;
                level += 1;
            }
        }
        let mut runs = levels.into_iter().flatten().collect::<Vec<_>>();
        while runs.len() > 1 {
            let mut next = Vec::new();
            let mut remaining = runs.into_iter();
            loop {
                let group = remaining.by_ref().take(MERGE_FAN_IN).collect::<Vec<_>>();
                if group.is_empty() {
                    break;
                }
                next.push(if group.len() == 1 {
                    group.into_iter().next().unwrap()
                } else {
                    merge_runs(&self.out_dir, group)?
                });
            }
            runs = next;
        }
        let path = runs.pop().map(|mut run| std::mem::take(&mut run.path));
        self.blocks.push(SignatureRowBlock {
            facility,
            signature_key,
            row_count: summary.row_count,
            path,
        });
        Ok(summary)
    }

    pub(super) fn write_table(
        mut self,
        operators: &[OperatorState],
        mask_words: usize,
    ) -> Result<BakeComboTableSummary> {
        self.blocks.sort_by(|a, b| {
            a.facility
                .cmp(b.facility)
                .then_with(|| a.signature_key.cmp(&b.signature_key))
        });
        let mut indexes = Vec::new();
        let mut row_count = 0usize;
        for block in &self.blocks {
            if block.row_count != 0 {
                indexes.push(BakedComboIndex {
                    signature_key: block.signature_key.clone(),
                    start: row_count,
                    len: block.row_count,
                });
                row_count += block.row_count;
            }
        }

        let path = self.out_dir.join("combo_table.bin");
        let mut writer = BufWriter::new(File::create(&path)?);
        let header = ComboTableHeader {
            schema_version: BAKE_SCHEMA_VERSION,
            generator: None,
            operator_count: operators.len(),
            operator_names: operators.iter().map(|state| state.name.as_str()).collect(),
            operator_states: operators,
            mask_words,
            indexes: &indexes,
            row_count,
        };
        encode_into(&mut writer, &header)?;
        for block in &self.blocks {
            if let Some(path) = &block.path {
                let mut reader = BufReader::new(File::open(path)?);
                std::io::copy(&mut reader, &mut writer)?;
            }
        }
        writer.flush()?;
        drop(writer);
        let bytes = fs::metadata(&path)?.len();
        Ok(BakeComboTableSummary {
            bytes,
            rows_by_signature: indexes
                .into_iter()
                .map(|index| (index.signature_key, index.len))
                .collect(),
        })
    }
}

const MERGE_FAN_IN: usize = 16;
const IO_BUFFER: usize = 256 * 1024;

#[derive(Default)]
pub(super) struct RowSummary {
    pub row_count: usize,
    pub mechanism_rows: usize,
    pub mechanism_signatures: BTreeSet<TradeRoomMechanismSignature>,
}

struct Run {
    path: PathBuf,
    rows: usize,
}
impl Drop for Run {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct MergeRow {
    row: BakedComboRowDisk,
    source: usize,
}
impl Ord for MergeRow {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // The heap returns the first row under the existing disk ordering.
        compare_disk_rows(&other.row, &self.row).then_with(|| other.source.cmp(&self.source))
    }
}
impl PartialOrd for MergeRow {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for MergeRow {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for MergeRow {}

fn compare_disk_rows(a: &BakedComboRowDisk, b: &BakedComboRowDisk) -> std::cmp::Ordering {
    super::compare_row_keys(
        a.sort_efficiency_millis,
        a.manufacture_storage_limit,
        &a.operator_indices,
        b.sort_efficiency_millis,
        b.manufacture_storage_limit,
        &b.operator_indices,
    )
}

fn merge_runs(out_dir: &Path, runs: Vec<Run>) -> Result<Run> {
    let (path, file) = create_row_block(out_dir)?;
    let result = Run {
        path,
        rows: runs.iter().map(|run| run.rows).sum(),
    };
    let mut writer = BufWriter::with_capacity(IO_BUFFER, file);
    let mut readers = runs
        .iter()
        .map(|run| {
            File::open(&run.path).map(|file| (BufReader::with_capacity(IO_BUFFER, file), run.rows))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut heap = BinaryHeap::new();
    for (source, (reader, remaining)) in readers.iter_mut().enumerate() {
        if *remaining > 0 {
            heap.push(MergeRow {
                row: decode_from(reader)?,
                source,
            });
            *remaining -= 1;
        }
    }
    while let Some(MergeRow { row, source }) = heap.pop() {
        encode_into(&mut writer, &row)?;
        let (reader, remaining) = &mut readers[source];
        if *remaining > 0 {
            heap.push(MergeRow {
                row: decode_from(reader)?,
                source,
            });
            *remaining -= 1;
        }
    }
    writer.flush()?;
    drop(writer);
    drop(readers);
    Ok(result)
}

// Field order and types encode exactly the BakedComboTableDisk prefix. Bincode
// writes structs without a header, and its Vec length is a fixed-width u64,
// which is also the encoding of row_count's usize. Borrowed slices/strings have
// the same encodings as the corresponding owned Vec/String metadata.
#[derive(Serialize)]
struct ComboTableHeader<'a> {
    schema_version: u32,
    generator: Option<BakeGeneratorFingerprint>,
    operator_count: usize,
    operator_names: Vec<&'a str>,
    operator_states: &'a [OperatorState],
    mask_words: usize,
    indexes: &'a [BakedComboIndex],
    row_count: usize,
}

fn encode_into(writer: impl Write, value: &impl Serialize) -> Result<()> {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize_into(writer, value)
        .map_err(|error| Error::msg(format!("encode combo_table.bin: {error}")))
}

fn create_row_block(out_dir: &Path) -> Result<(PathBuf, File)> {
    static NEXT_BLOCK: AtomicU64 = AtomicU64::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    loop {
        let sequence = NEXT_BLOCK.fetch_add(1, Ordering::Relaxed);
        let path = out_dir.join(format!(
            ".combo-rows-{}-{timestamp}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

#[derive(Deserialize)]
struct OwnedComboTableHeader {
    schema_version: u32,
    generator: Option<BakeGeneratorFingerprint>,
    operator_count: usize,
    operator_names: Vec<String>,
    operator_states: Vec<OperatorState>,
    mask_words: usize,
    indexes: Vec<BakedComboIndex>,
    row_count: usize,
}

pub(super) fn decode_from<T: serde::de::DeserializeOwned>(reader: impl Read) -> Result<T> {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .deserialize_from(reader)
        .map_err(|error| Error::msg(format!("read combo_table.bin: {error}")))
}

pub(super) fn read_header(reader: impl Read) -> Result<(BakedComboTableDisk, usize)> {
    let header: OwnedComboTableHeader = decode_from(reader)?;
    Ok((
        BakedComboTableDisk {
            schema_version: header.schema_version,
            generator: header.generator,
            operator_count: header.operator_count,
            operator_names: header.operator_names,
            operator_states: header.operator_states,
            mask_words: header.mask_words,
            indexes: header.indexes,
            rows: Vec::new(),
        },
        header.row_count,
    ))
}
