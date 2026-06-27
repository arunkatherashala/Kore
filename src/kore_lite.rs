use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KType {
	Int = 1,
	Float = 2,
	Bool = 3,
	Str = 4,
	Bytes = 5,
}

#[derive(Debug, Clone)]
pub enum KVal {
	Int(i64),
	Float(f64),
	Bool(bool),
	Str(String),
	Bytes(Vec<u8>),
	Null,
}

#[derive(Debug, Clone)]
pub struct KColumn {
	pub name: String,
	pub ktype: KType,
}

impl KColumn {
	pub fn new(name: &str, ktype: KType) -> Self {
		Self {
			name: name.to_string(),
			ktype,
		}
	}
}

pub struct KoreWriter {
	pub columns: Vec<KColumn>,
}

impl KoreWriter {
	pub fn new(columns: Vec<KColumn>) -> Self {
		Self { columns }
	}

	pub fn write(&self, path: &str, rows: &[Vec<KVal>]) -> Result<String, String> {
		if self.columns.is_empty() {
			return Err("No columns provided".to_string());
		}

		let mut out: Vec<u8> = Vec::new();
		out.extend_from_slice(b"KORELITE");
		out.push(1u8);

		out.extend_from_slice(&(self.columns.len() as u32).to_le_bytes());
		out.extend_from_slice(&(rows.len() as u64).to_le_bytes());

		for c in &self.columns {
			let name_bytes = c.name.as_bytes();
			out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
			out.extend_from_slice(name_bytes);
			out.push(c.ktype as u8);
		}

		for row in rows {
			if row.len() != self.columns.len() {
				return Err("Row length does not match schema column count".to_string());
			}
			for v in row {
				match v {
					KVal::Null => {
						out.push(0u8);
					}
					KVal::Int(n) => {
						out.push(1u8);
						out.extend_from_slice(&n.to_le_bytes());
					}
					KVal::Float(f) => {
						out.push(2u8);
						out.extend_from_slice(&f.to_le_bytes());
					}
					KVal::Bool(b) => {
						out.push(3u8);
						out.push(if *b { 1 } else { 0 });
					}
					KVal::Str(s) => {
						out.push(4u8);
						let bytes = s.as_bytes();
						out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
						out.extend_from_slice(bytes);
					}
					KVal::Bytes(b) => {
						out.push(5u8);
						out.extend_from_slice(&(b.len() as u32).to_le_bytes());
						out.extend_from_slice(b);
					}
				}
			}
		}

		fs::write(path, &out).map_err(|e| format!("Cannot write {}: {}", path, e))?;
		Ok(format!(
			"KORE-LITE: {} rows x {} cols -> {} bytes",
			rows.len(),
			self.columns.len(),
			out.len()
		))
	}
}
