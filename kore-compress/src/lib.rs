//! KORE Layer 45 — Column Compression
//!
//! Three complementary codecs — automatically selected per column:
//!
//! | Codec             | Best for                        | Typical ratio |
//! |-------------------|---------------------------------|---------------|
//! | DictionaryEncoded | Low-cardinality strings / ints  | 5–20×         |
//! | RunLengthEncoded  | Sorted / repetitive sequences   | 2–100×        |
//! | BitPacked         | Small non-negative integers     | 2–8×          |
//!
//! The `CompressedBlock` wrapper stores each column with its chosen codec.
//! `decompress()` reconstructs a full `DataBlock` transparently.

use kore_core::{Column, ColumnData, DataBlock, KoreError};

// ─── Dictionary encoding ──────────────────────────────────────────────────────

/// Replaces every distinct string with a compact integer code.
/// Nulls are stored as `None<u32>`.
#[derive(Debug, Clone)]
pub struct DictEncoded {
    pub dict:  Vec<String>,          // index → value
    pub codes: Vec<Option<u32>>,     // one code per row
}

impl DictEncoded {
    pub fn encode(data: &[Option<String>]) -> Self {
        let mut dict: Vec<String> = Vec::new();
        let mut idx: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let codes = data.iter().map(|x| {
            x.as_ref().map(|s| {
                if let Some(&c) = idx.get(s) { c }
                else {
                    let c = dict.len() as u32;
                    idx.insert(s.clone(), c);
                    dict.push(s.clone());
                    c
                }
            })
        }).collect();
        DictEncoded { dict, codes }
    }

    pub fn decode(&self) -> Vec<Option<String>> {
        self.codes.iter().map(|x| {
            x.and_then(|c| self.dict.get(c as usize).cloned())
        }).collect()
    }

    /// Estimated bytes saved (original - compressed).
    pub fn compress_ratio(&self) -> f64 {
        if self.codes.is_empty() { return 1.0; }
        let orig_bytes: usize = self.codes.iter().enumerate()
            .map(|(i, x)| x.as_ref()
                .and_then(|&c| self.dict.get(c as usize))
                .map(|s| s.len() + 8)
                .unwrap_or(8))
            .sum();
        let comp_bytes = self.dict.iter().map(|s| s.len()).sum::<usize>()
            + self.codes.len() * 4;   // 4 bytes per code
        if comp_bytes == 0 { 1.0 } else { orig_bytes as f64 / comp_bytes as f64 }
    }
}

// ─── Run-length encoding ──────────────────────────────────────────────────────

/// Stores (value, count) pairs.  Most effective on sorted or repetitive data.
#[derive(Debug, Clone)]
pub struct RleEncoded<T: Clone + PartialEq> {
    pub runs: Vec<(Option<T>, usize)>,
}

impl<T: Clone + PartialEq> RleEncoded<T> {
    pub fn encode(data: &[Option<T>]) -> Self {
        let mut runs: Vec<(Option<T>, usize)> = Vec::new();
        for x in data {
            if let Some((last, cnt)) = runs.last_mut() {
                if last == x { *cnt += 1; continue; }
            }
            runs.push((x.clone(), 1));
        }
        RleEncoded { runs }
    }

    pub fn decode(&self) -> Vec<Option<T>> {
        let mut out = Vec::new();
        for (val, cnt) in &self.runs {
            for _ in 0..*cnt { out.push(val.clone()); }
        }
        out
    }

    pub fn run_count(&self) -> usize { self.runs.len() }

    /// Compression ratio: original element count / run count.
    pub fn compress_ratio(&self) -> f64 {
        let total: usize = self.runs.iter().map(|(_, c)| c).sum();
        if self.runs.is_empty() { 1.0 } else { total as f64 / self.runs.len() as f64 }
    }
}

// ─── Bit packing ──────────────────────────────────────────────────────────────

/// Packs `bits_per_value`-bit unsigned integers into 64-bit words.
/// Values must fit in `bits_per_value` bits.
#[derive(Debug, Clone)]
pub struct BitPacked {
    pub bits_per_value: u8,
    pub n_values:       usize,
    pub packed:         Vec<u64>,
}

impl BitPacked {
    /// Number of bits needed to represent `max_val`.
    pub fn bits_needed(max_val: u64) -> u8 {
        if max_val == 0 { 1 } else { (64 - max_val.leading_zeros()) as u8 }
    }

    pub fn pack(values: &[u64], bits: u8) -> Self {
        let bits = bits.max(1).min(63);
        let mask  = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
        let total_bits = values.len() * bits as usize;
        let n_words    = (total_bits + 63) / 64;
        let mut packed = vec![0u64; n_words];
        for (i, &v) in values.iter().enumerate() {
            let bit_pos  = i * bits as usize;
            let word_idx = bit_pos / 64;
            let bit_off  = bit_pos % 64;
            packed[word_idx] |= (v & mask) << bit_off;
            // Handle split across two words
            if bit_off + bits as usize > 64 {
                packed[word_idx + 1] |= (v & mask) >> (64 - bit_off);
            }
        }
        BitPacked { bits_per_value: bits, n_values: values.len(), packed }
    }

    pub fn unpack(&self) -> Vec<u64> {
        let bits = self.bits_per_value as usize;
        let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
        (0..self.n_values).map(|i| {
            let bit_pos  = i * bits;
            let word_idx = bit_pos / 64;
            let bit_off  = bit_pos % 64;
            let mut val  = (self.packed[word_idx] >> bit_off) & mask;
            if bit_off + bits > 64 && word_idx + 1 < self.packed.len() {
                val |= (self.packed[word_idx + 1] << (64 - bit_off)) & mask;
            }
            val
        }).collect()
    }

    /// Bits used per value vs 64 bits.
    pub fn compress_ratio(&self) -> f64 { 64.0 / self.bits_per_value as f64 }
}

// ─── Compressed column variants ───────────────────────────────────────────────

pub enum CompressedCol {
    /// Plain (uncompressed) — used when compression doesn't help.
    Plain(Column),
    DictStr(DictEncoded, String),          // dict-encoded strings + col name
    RleI64(RleEncoded<i64>, String),       // RLE integers
    RleF64(RleEncoded<f64_wrap>, String),  // RLE floats (wrapped for PartialEq)
    BitPackedI64 {
        packed:    BitPacked,
        offset:    i64,   // minimum value (all stored values = actual - offset)
        col_name:  String,
    },
}

/// Wrapper to make f64 PartialEq by bit representation (NaN-safe for our use).
#[derive(Clone, Debug, PartialEq)]
pub struct f64_wrap(pub u64);
impl f64_wrap {
    pub fn from(f: f64)  -> Self { Self(f.to_bits()) }
    pub fn value(&self)  -> f64  { f64::from_bits(self.0) }
}

// ─── Compressed block ─────────────────────────────────────────────────────────

pub struct CompressedBlock {
    pub num_rows: usize,
    pub columns:  Vec<CompressedCol>,
}

/// Automatically compress a DataBlock — choose the best codec per column.
pub fn auto_compress(block: &DataBlock) -> CompressedBlock {
    let columns = block.columns.iter().map(|col| {
        compress_column(col)
    }).collect();
    CompressedBlock { num_rows: block.num_rows, columns }
}

fn compress_column(col: &Column) -> CompressedCol {
    match &col.data {
        // ── Strings → Dictionary encoding ───────────────────────────────────
        ColumnData::Str(v) => {
            let enc = DictEncoded::encode(v);
            if enc.compress_ratio() > 1.5 {
                CompressedCol::DictStr(enc, col.name.clone())
            } else {
                CompressedCol::Plain(col.clone())
            }
        }
        // ── Integers → RLE (if sorted/repetitive) or bit-packing (if small) ─
        ColumnData::Int64(v) => {
            let rle = RleEncoded::encode(v);
            let rle_ratio = rle.compress_ratio();

            // Try bit-packing if values are small non-negative
            let vals: Vec<i64> = v.iter().filter_map(|x| *x).collect();
            if !vals.is_empty() {
                let min = *vals.iter().min().unwrap();
                let max = *vals.iter().max().unwrap();
                if min >= 0 {
                    let bits = BitPacked::bits_needed(max as u64);
                    let ratio = 64.0 / bits as f64;
                    if ratio > 2.0 && ratio >= rle_ratio {
                        let u64_vals: Vec<u64> = v.iter().map(|x| x.unwrap_or(0) as u64).collect();
                        return CompressedCol::BitPackedI64 {
                            packed:   BitPacked::pack(&u64_vals, bits),
                            offset:   0,
                            col_name: col.name.clone(),
                        };
                    }
                } else if max - min < i64::MAX {
                    let offset = min;
                    let range  = (max - min) as u64;
                    let bits   = BitPacked::bits_needed(range);
                    let ratio  = 64.0 / bits as f64;
                    if ratio > 2.0 && ratio >= rle_ratio {
                        let u64_vals: Vec<u64> = v.iter().map(|x| (x.unwrap_or(min) - min) as u64).collect();
                        return CompressedCol::BitPackedI64 {
                            packed:   BitPacked::pack(&u64_vals, bits),
                            offset,
                            col_name: col.name.clone(),
                        };
                    }
                }
                if rle_ratio > 2.0 {
                    return CompressedCol::RleI64(rle, col.name.clone());
                }
            }
            CompressedCol::Plain(col.clone())
        }
        // ── Floats → RLE (if repetitive) ────────────────────────────────────
        ColumnData::Float64(v) => {
            let wrapped: Vec<Option<f64_wrap>> = v.iter().map(|x| x.map(f64_wrap::from)).collect();
            let rle = RleEncoded::encode(&wrapped);
            if rle.compress_ratio() > 2.0 {
                CompressedCol::RleF64(rle, col.name.clone())
            } else {
                CompressedCol::Plain(col.clone())
            }
        }
        _ => CompressedCol::Plain(col.clone()),
    }
}

/// Decompress a `CompressedBlock` back to a plain `DataBlock`.
pub fn decompress(cb: &CompressedBlock) -> DataBlock {
    let columns = cb.columns.iter().map(|cc| {
        match cc {
            CompressedCol::Plain(c) => c.clone(),
            CompressedCol::DictStr(enc, name) => Column {
                name: name.clone(), data: ColumnData::Str(enc.decode()),
            },
            CompressedCol::RleI64(rle, name) => Column {
                name: name.clone(), data: ColumnData::Int64(rle.decode()),
            },
            CompressedCol::RleF64(rle, name) => Column {
                name: name.clone(),
                data: ColumnData::Float64(rle.decode().into_iter().map(|x| x.map(|w| w.value())).collect()),
            },
            CompressedCol::BitPackedI64 { packed, offset, col_name } => {
                let vals: Vec<Option<i64>> = packed.unpack().into_iter()
                    .map(|u| Some(u as i64 + offset))
                    .collect();
                Column { name: col_name.clone(), data: ColumnData::Int64(vals) }
            }
        }
    }).collect();
    DataBlock { columns, num_rows: cb.num_rows }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    #[test]
    fn test_dict_encode_decode() {
        let data = vec![Some("A".into()), Some("B".into()), Some("A".into()), None, Some("C".into())];
        let enc = DictEncoded::encode(&data);
        assert_eq!(enc.dict.len(), 3);
        let decoded = enc.decode();
        assert_eq!(decoded, data);
        assert!(enc.compress_ratio() > 1.0);
    }

    #[test]
    fn test_rle_i64() {
        let data: Vec<Option<i64>> = vec![
            Some(1),Some(1),Some(1),Some(2),Some(2),Some(3),None,None
        ];
        let enc = RleEncoded::encode(&data);
        assert_eq!(enc.run_count(), 4); // (1,3),(2,2),(3,1),(None,2)
        assert_eq!(enc.decode(), data);
        assert!(enc.compress_ratio() > 1.5);
    }

    #[test]
    fn test_bit_packing() {
        let vals: Vec<u64> = vec![0, 1, 2, 3, 4, 5, 6, 7]; // 3 bits each
        let packed = BitPacked::pack(&vals, 3);
        let unpacked = packed.unpack();
        assert_eq!(unpacked, vals);
        assert!(packed.compress_ratio() > 5.0);
    }

    #[test]
    fn test_bit_packing_large() {
        let vals: Vec<u64> = (0..200).map(|i| i % 16).collect(); // 4 bits each
        let packed = BitPacked::pack(&vals, 4);
        assert_eq!(packed.unpack(), vals);
    }

    #[test]
    fn test_auto_compress_decompress_roundtrip() {
        let block = DataBlock {
            num_rows: 20,
            columns: vec![
                // Strings with repetition → dict
                Column { name: "cat".into(), data: ColumnData::Str(
                    (0..20).map(|i| Some(format!("region_{}", i % 3))).collect()
                )},
                // Small integers → bit-pack
                Column { name: "flag".into(), data: ColumnData::Int64(
                    (0..20).map(|i| Some(i % 4)).collect()
                )},
                // Repetitive floats → RLE
                Column { name: "rate".into(), data: ColumnData::Float64(
                    (0..20).map(|_| Some(1.5)).collect()
                )},
            ],
        };

        let cb = auto_compress(&block);
        let restored = decompress(&cb);
        assert_eq!(restored.num_rows, block.num_rows);

        // Validate data integrity
        assert_eq!(restored.columns[0].data, block.columns[0].data);
        assert_eq!(restored.columns[1].data, block.columns[1].data);
        // Float comparison (RLE path)
        if let (ColumnData::Float64(a), ColumnData::Float64(b)) =
            (&restored.columns[2].data, &block.columns[2].data) {
            for (x, y) in a.iter().zip(b.iter()) {
                assert_eq!(x, y);
            }
        }
    }
}
