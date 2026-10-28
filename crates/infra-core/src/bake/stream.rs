//! Generation-only row blocks keep at most one signature's rows in memory.

use std::fs::{self, File, OpenOptions};
use std::hash::Hasher;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bincode::Options;
use serde::{Deserialize, Serialize};

use super::{
    BakeComboTableFingerprint, BakeGeneratorFingerprint, BakedComboIndex, BakedComboRow,
    BakedComboRowDisk, BakedComboTableDisk, OperatorState, BAKE_SCHEMA_VERSION,
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

    pub(super) fn push(
        &mut self,
        facility: &'static str,
        signature_key: String,
        rows: Vec<BakedComboRow>,
    ) -> Result<()> {
        let mut block = SignatureRowBlock {
            facility,
            signature_key,
            row_count: rows.len(),
            path: None,
        };
        if !rows.is_empty() {
            let (path, file) = create_row_block(&self.out_dir)?;
            block.path = Some(path);
            let mut writer = BufWriter::new(file);
            for row in rows {
                let disk = BakedComboRowDisk::from(&row);
                drop(row);
                // A block contains only row encodings, without a Vec length.
                encode_into(&mut writer, &disk)?;
            }
            writer.flush()?;
        }
        self.blocks.push(block);
        Ok(())
    }

    pub(super) fn write_table(
        mut self,
        operators: &[OperatorState],
        mask_words: usize,
        generator: &Option<BakeGeneratorFingerprint>,
    ) -> Result<BakeComboTableFingerprint> {
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
            generator,
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
        let (bytes, hash64) = hash_file(&path)?;
        Ok(BakeComboTableFingerprint {
            bytes,
            hash64,
            rows_by_signature: indexes
                .into_iter()
                .map(|index| (index.signature_key, index.len))
                .collect(),
        })
    }
}

// Field order and types encode exactly the BakedComboTableDisk prefix. Bincode
// writes structs without a header, and its Vec length is a fixed-width u64,
// which is also the encoding of row_count's usize. Borrowed slices/strings have
// the same encodings as the corresponding owned Vec/String metadata.
#[derive(Serialize)]
struct ComboTableHeader<'a> {
    schema_version: u32,
    generator: &'a Option<BakeGeneratorFingerprint>,
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

fn hash_file(path: &Path) -> Result<(u64, String)> {
    hash_open_file(&mut File::open(path)?)
}

/// Hash and rewind one open immutable generation file, preserving slice Hash.
pub(super) fn hash_open_file(file: &mut File) -> Result<(u64, String)> {
    file.seek(SeekFrom::Start(0))?;
    let bytes = file.metadata()?.len();
    let len =
        usize::try_from(bytes).map_err(|_| Error::msg("combo_table.bin length exceeds usize"))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Preserve hash_bytes' [u8] Hash contract: hash the full length once, then
    // feed all bytes in order. Hashing each chunk as a slice changes the hash.
    hasher.write_usize(len);
    let mut reader = BufReader::new(&mut *file);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.write(&buffer[..read]);
    }
    drop(reader);
    file.seek(SeekFrom::Start(0))?;
    Ok((bytes, format!("{:016x}", hasher.finish())))
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
