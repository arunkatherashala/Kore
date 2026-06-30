use std::cmp::Ordering;
use serde::{Deserialize, Serialize};
use rayon::prelude::*;
use crate::KoreError;

// ─── Column storage ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColumnData {
    Int64(Vec<Option<i64>>),
    Float64(Vec<Option<f64>>),
    Bool(Vec<Option<bool>>),
    Str(Vec<Option<String>>),
    /// Dictionary-encoded strings: codes[row] = index into dict (u8::MAX = NULL).
    /// Zero heap-pointer chasing in hot loops — 24× less memory than Vec<Option<String>>.
    /// Use Column::str_dict() to create. Maximum 254 distinct non-NULL values.
    StrDict { codes: Vec<u8>, dict: Vec<String> },
}

impl ColumnData {
    pub fn len(&self) -> usize {
        match self {
            Self::Int64(v)       => v.len(),
            Self::Float64(v)     => v.len(),
            Self::Bool(v)        => v.len(),
            Self::Str(v)         => v.len(),
            Self::StrDict { codes, .. } => codes.len(),
        }
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    pub fn dtype(&self) -> DataType {
        match self {
            Self::Int64(_)         => DataType::Int64,
            Self::Float64(_)       => DataType::Float64,
            Self::Bool(_)          => DataType::Bool,
            Self::Str(_)           => DataType::Str,
            Self::StrDict { .. }   => DataType::Str,
        }
    }

    /// Get the string value at `row` for both Str and StrDict variants.
    #[inline]
    pub fn get_str(&self, row: usize) -> Option<&str> {
        match self {
            Self::Str(v) => v.get(row).and_then(|x| x.as_deref()),
            Self::StrDict { codes, dict } => {
                let c = *codes.get(row)?;
                if c == u8::MAX { None } else { dict.get(c as usize).map(|s| s.as_str()) }
            }
            _ => None,
        }
    }

    pub fn get_value(&self, idx: usize) -> Value {
        match self {
            Self::Int64(v)   => v.get(idx).and_then(|x| x.as_ref()).map(|&x| Value::Int(x)).unwrap_or(Value::Null),
            Self::Float64(v) => v.get(idx).and_then(|x| x.as_ref()).map(|&x| Value::Float(x)).unwrap_or(Value::Null),
            Self::Bool(v)    => v.get(idx).and_then(|x| x.as_ref()).map(|&x| Value::Bool(x)).unwrap_or(Value::Null),
            Self::Str(v)     => v.get(idx).and_then(|x| x.as_ref()).map(|x| Value::Str(x.clone())).unwrap_or(Value::Null),
            Self::StrDict { codes, dict } => {
                let c = codes.get(idx).copied().unwrap_or(u8::MAX);
                if c == u8::MAX { Value::Null } else { dict.get(c as usize).map(|s| Value::Str(s.clone())).unwrap_or(Value::Null) }
            }
        }
    }

    pub fn empty_like(&self) -> Self {
        match self {
            Self::Int64(_)       => Self::Int64(vec![]),
            Self::Float64(_)     => Self::Float64(vec![]),
            Self::Bool(_)        => Self::Bool(vec![]),
            Self::Str(_)         => Self::Str(vec![]),
            Self::StrDict { dict, .. } => Self::StrDict { codes: vec![], dict: dict.clone() },
        }
    }

    pub fn append_value(&mut self, value: &Value) -> Result<(), KoreError> {
        match (self, value) {
            (Self::Int64(v),   Value::Int(i))   => { v.push(Some(*i)); Ok(()) }
            (Self::Int64(v),   Value::Null)     => { v.push(None); Ok(()) }
            (Self::Float64(v), Value::Float(f)) => { v.push(Some(*f)); Ok(()) }
            (Self::Float64(v), Value::Int(i))   => { v.push(Some(*i as f64)); Ok(()) }
            (Self::Float64(v), Value::Null)     => { v.push(None); Ok(()) }
            (Self::Bool(v),    Value::Bool(b))  => { v.push(Some(*b)); Ok(()) }
            (Self::Bool(v),    Value::Null)     => { v.push(None); Ok(()) }
            (Self::Str(v),     Value::Str(s))   => { v.push(Some(s.clone())); Ok(()) }
            (Self::Str(v),     Value::Null)     => { v.push(None); Ok(()) }
            (Self::StrDict { codes, dict }, Value::Str(s)) => {
                let code = dict.iter().position(|d| d == s)
                    .unwrap_or_else(|| { dict.push(s.clone()); dict.len() - 1 }) as u8;
                codes.push(code); Ok(())
            }
            (Self::StrDict { codes, .. }, Value::Null) => { codes.push(u8::MAX); Ok(()) }
            (col, val) => Err(KoreError::TypeMismatch {
                expected: col.dtype().to_string(),
                got: val.type_name().to_string(),
            }),
        }
    }

    /// Extract rows at given indices (preserves type)
    pub fn take_rows(&self, indices: &[usize]) -> Self {
        match self {
            Self::Int64(v)   => Self::Int64(indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
            Self::Float64(v) => Self::Float64(indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
            Self::Bool(v)    => Self::Bool(indices.iter().map(|&i| v.get(i).copied().flatten()).collect()),
            Self::Str(v)     => Self::Str(indices.iter().map(|&i| v.get(i).and_then(|x| x.clone())).collect()),
            Self::StrDict { codes, dict } => Self::StrDict {
                codes: indices.iter().map(|&i| codes.get(i).copied().unwrap_or(u8::MAX)).collect(),
                dict: dict.clone(),
            },
        }
    }
}

// ─── Runtime value ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Null,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_)   => "Int64",
            Value::Float(_) => "Float64",
            Value::Bool(_)  => "Bool",
            Value::Str(_)   => "Str",
            Value::Null     => "Null",
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i)   => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _               => None,
        }
    }
}

// ─── Hashable join key (Float excluded — NaN breaks Hash) ──────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JoinKey {
    Int(i64),
    Bool(bool),
    Str(String),
    Null,
}

impl From<&Value> for JoinKey {
    fn from(v: &Value) -> Self {
        match v {
            Value::Int(i)  => JoinKey::Int(*i),
            Value::Bool(b) => JoinKey::Bool(*b),
            Value::Str(s)  => JoinKey::Str(s.clone()),
            _              => JoinKey::Null,
        }
    }
}

pub fn compare_join_keys(a: &JoinKey, b: &JoinKey) -> Ordering {
    use JoinKey::*;
    match (a, b) {
        (Int(x),  Int(y))  => x.cmp(y),
        (Bool(x), Bool(y)) => x.cmp(y),
        (Str(x),  Str(y))  => x.cmp(y),
        (Null,    Null)    => Ordering::Equal,
        (Null,    _)       => Ordering::Less,
        (_,       Null)    => Ordering::Greater,
        _                  => Ordering::Equal,
    }
}

// ─── DataType ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType { Int64, Float64, Bool, Str }

impl std::fmt::Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataType::Int64   => write!(f, "Int64"),
            DataType::Float64 => write!(f, "Float64"),
            DataType::Bool    => write!(f, "Bool"),
            DataType::Str     => write!(f, "Str"),
        }
    }
}

// ─── Join type ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinType { Inner, Left, Right, Full }

// ─── Column ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data: ColumnData,
}

impl Column {
    pub fn int64(name: &str, data: Vec<Option<i64>>) -> Self {
        Self { name: name.into(), data: ColumnData::Int64(data) }
    }
    pub fn float64(name: &str, data: Vec<Option<f64>>) -> Self {
        Self { name: name.into(), data: ColumnData::Float64(data) }
    }
    pub fn str_col(name: &str, data: Vec<Option<String>>) -> Self {
        Self { name: name.into(), data: ColumnData::Str(data) }
    }
    pub fn bool_col(name: &str, data: Vec<Option<bool>>) -> Self {
        Self { name: name.into(), data: ColumnData::Bool(data) }
    }
    /// Create a dictionary-encoded string column.
    /// `codes[row]` is an index into `dict`; u8::MAX = NULL.
    pub fn str_dict(name: &str, codes: Vec<u8>, dict: Vec<String>) -> Self {
        Self { name: name.into(), data: ColumnData::StrDict { codes, dict } }
    }
}

// ─── DataBlock ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBlock {
    pub columns: Vec<Column>,
    pub num_rows: usize,
}

impl DataBlock {
    pub fn new(columns: Vec<Column>) -> Result<Self, KoreError> {
        let num_rows = if columns.is_empty() {
            0
        } else {
            let n = columns[0].data.len();
            for col in &columns[1..] {
                if col.data.len() != n {
                    return Err(KoreError::SchemaMismatch(format!(
                        "column '{}': expected {} rows, got {}",
                        col.name, n, col.data.len()
                    )));
                }
            }
            n
        };
        Ok(Self { columns, num_rows })
    }

    pub fn empty() -> Self { Self { columns: vec![], num_rows: 0 } }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn column_idx(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    pub fn join_key(&self, row: usize, key_col: &str) -> Result<JoinKey, KoreError> {
        let col = self.column(key_col)
            .ok_or_else(|| KoreError::ColumnNotFound(key_col.into()))?;
        Ok(JoinKey::from(&col.data.get_value(row)))
    }

    /// Select specific rows by index (used by joins and sorts)
    pub fn select_rows(&self, indices: &[usize]) -> Self {
        let columns = self.columns.iter().map(|c| Column {
            name: c.name.clone(),
            data: c.data.take_rows(indices),
        }).collect();
        Self { columns, num_rows: indices.len() }
    }

    /// Sort by a column
    pub fn sort_by(&self, col_name: &str, ascending: bool) -> Result<Self, KoreError> {
        let col_idx = self.column_idx(col_name)
            .ok_or_else(|| KoreError::ColumnNotFound(col_name.into()))?;
        let data = &self.columns[col_idx].data;

        // Cache-friendly Schwartzian transform: co-locate key+index so the
        // sort comparator accesses a single contiguous (key, idx) array instead
        // of two separate arrays with scattered random accesses.
        // Parallel sort: use Rayon par_sort_unstable_by for 8× speedup on 6M+ rows
        let indices: Vec<usize> = match data {
            ColumnData::Float64(v) => {
                let mut pairs: Vec<(f64, usize)> = v.par_iter()
                    .enumerate()
                    .map(|(i, opt)| (opt.unwrap_or(f64::MAX), i))
                    .collect();
                if ascending {
                    pairs.par_sort_unstable_by(|(a,_),(b,_)| a.partial_cmp(b).unwrap_or(Ordering::Equal));
                } else {
                    pairs.par_sort_unstable_by(|(a,_),(b,_)| b.partial_cmp(a).unwrap_or(Ordering::Equal));
                }
                pairs.into_iter().map(|(_,i)| i).collect()
            }
            ColumnData::Int64(v) => {
                let mut pairs: Vec<(i64, usize)> = v.par_iter()
                    .enumerate()
                    .map(|(i, opt)| (opt.unwrap_or(i64::MIN), i))
                    .collect();
                if ascending {
                    pairs.par_sort_unstable_by_key(|&(k,_)| k);
                } else {
                    pairs.par_sort_unstable_by(|(a,_),(b,_)| b.cmp(a));
                }
                pairs.into_iter().map(|(_,i)| i).collect()
            }
            ColumnData::Bool(v) => {
                let mut pairs: Vec<(u8, usize)> = v.iter()
                    .enumerate()
                    .map(|(i, opt)| (opt.map_or(0, |b| b as u8), i))
                    .collect();
                if ascending { pairs.sort_unstable_by_key(|&(k,_)| k); }
                else          { pairs.sort_unstable_by(|(a,_),(b,_)| b.cmp(a)); }
                pairs.into_iter().map(|(_,i)| i).collect()
            }
            ColumnData::Str(v) => {
                let mut indices: Vec<usize> = (0..self.num_rows).collect();
                indices.sort_unstable_by(|&a, &b| {
                    let sa = v[a].as_deref().unwrap_or("");
                    let sb = v[b].as_deref().unwrap_or("");
                    if ascending { sa.cmp(sb) } else { sb.cmp(sa) }
                });
                indices
            }
            ColumnData::StrDict { codes, dict } => {
                let mut indices: Vec<usize> = (0..self.num_rows).collect();
                indices.sort_unstable_by(|&a, &b| {
                    let ca = codes[a]; let cb = codes[b];
                    let sa = if ca == u8::MAX { "" } else { dict.get(ca as usize).map(|s| s.as_str()).unwrap_or("") };
                    let sb = if cb == u8::MAX { "" } else { dict.get(cb as usize).map(|s| s.as_str()).unwrap_or("") };
                    if ascending { sa.cmp(sb) } else { sb.cmp(sa) }
                });
                indices
            }
        };

        Ok(self.select_rows(&indices))
    }

    /// Vertically concatenate blocks with matching schemas
    pub fn concat(blocks: Vec<DataBlock>) -> Result<Self, KoreError> {
        if blocks.is_empty() { return Ok(Self::empty()); }
        let schema: Vec<(String, DataType)> = blocks[0].columns.iter()
            .map(|c| (c.name.clone(), c.data.dtype()))
            .collect();
        let mut acc: Vec<ColumnData> = schema.iter()
            .map(|(_, dt)| match dt {
                DataType::Int64   => ColumnData::Int64(vec![]),
                DataType::Float64 => ColumnData::Float64(vec![]),
                DataType::Bool    => ColumnData::Bool(vec![]),
                DataType::Str     => {
                    // Check if first block has StrDict — preserve variant
                    let first_col = blocks[0].columns.iter().find(|(c)| c.data.dtype() == DataType::Str);
                    if let Some(c) = first_col {
                        if let ColumnData::StrDict { dict, .. } = &c.data {
                            return ColumnData::StrDict { codes: vec![], dict: dict.clone() };
                        }
                    }
                    ColumnData::Str(vec![])
                }
            })
            .collect();
        let mut total = 0usize;
        for block in &blocks {
            if block.columns.len() != schema.len() {
                return Err(KoreError::SchemaMismatch(
                    format!("expected {} columns, got {}", schema.len(), block.columns.len())
                ));
            }
            for (i, col) in block.columns.iter().enumerate() {
                match (&mut acc[i], &col.data) {
                    (ColumnData::Int64(d),   ColumnData::Int64(s))   => d.extend_from_slice(s),
                    (ColumnData::Float64(d), ColumnData::Float64(s)) => d.extend_from_slice(s),
                    (ColumnData::Bool(d),    ColumnData::Bool(s))    => d.extend_from_slice(s),
                    (ColumnData::Str(d),     ColumnData::Str(s))     => d.extend_from_slice(s),
                    // StrDict concat: if same dict, just extend codes; else convert to Str
                    (ColumnData::StrDict { codes: dc, dict: dd }, ColumnData::StrDict { codes: sc, dict: sd }) => {
                        if dd == sd {
                            dc.extend_from_slice(sc);
                        } else {
                            // Remap codes to shared dict
                            let base = dd.len();
                            for s in sd.iter() {
                                if !dd.contains(s) { dd.push(s.clone()); }
                            }
                            for &c in sc.iter() {
                                let new_code = if c == u8::MAX { u8::MAX } else {
                                    sd.get(c as usize).and_then(|s| dd.iter().position(|d| d == s)).unwrap_or(0) as u8
                                };
                                dc.push(new_code);
                            }
                        }
                    }
                    // StrDict with Str: convert StrDict to Str and extend
                    (ColumnData::Str(d), ColumnData::StrDict { codes, dict }) => {
                        d.extend(codes.iter().map(|&c| if c == u8::MAX { None } else { dict.get(c as usize).cloned() }));
                    }
                    _ => return Err(KoreError::TypeMismatch {
                        expected: schema[i].1.to_string(),
                        got: col.data.dtype().to_string(),
                    }),
                }
            }
            total += block.num_rows;
        }
        let columns = schema.into_iter().zip(acc)
            .map(|((name, _), data)| Column { name, data })
            .collect();
        Ok(Self { columns, num_rows: total })
    }

    /// Convert named float/int columns to a row-major feature matrix (for ML)
    pub fn to_feature_matrix(&self, feature_cols: &[&str]) -> Result<Vec<Vec<f64>>, KoreError> {
        let mut mat = vec![vec![0.0f64; feature_cols.len()]; self.num_rows];
        for (j, &name) in feature_cols.iter().enumerate() {
            let col = self.column(name)
                .ok_or_else(|| KoreError::ColumnNotFound(name.into()))?;
            for i in 0..self.num_rows {
                mat[i][j] = col.data.get_value(i).as_f64().unwrap_or(0.0);
            }
        }
        Ok(mat)
    }

    /// Extract a target column as Vec<f64>
    pub fn to_target_vector(&self, target_col: &str) -> Result<Vec<f64>, KoreError> {
        let col = self.column(target_col)
            .ok_or_else(|| KoreError::ColumnNotFound(target_col.into()))?;
        Ok((0..self.num_rows).map(|i| col.data.get_value(i).as_f64().unwrap_or(0.0)).collect())
    }
}

fn compare_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Int(x),   Value::Int(y))   => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Str(x),   Value::Str(y))   => x.cmp(y),
        (Value::Bool(x),  Value::Bool(y))  => x.cmp(y),
        (Value::Null,     Value::Null)     => Ordering::Equal,
        (Value::Null,     _)               => Ordering::Less,
        (_,               Value::Null)     => Ordering::Greater,
        _                                  => Ordering::Equal,
    }
}
