use std::io::{Read, Seek};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnStats {
    pub min: Option<String>,
    pub max: Option<String>,
    pub null_count: u64,
}

/// Compact binary representation of per-row-group metadata appended by `KoreWriter`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowGroupMetadata {
    pub row_count: u64,
    pub column_stats: Vec<ColumnStats>,
}

impl RowGroupMetadata {
    pub fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut cursor = std::io::Cursor::new(bytes);
        let mut buf8 = [0u8; 8];
        let mut buf4 = [0u8; 4];

        cursor.read_exact(&mut buf8)?;
        let row_count = u64::from_le_bytes(buf8);

        cursor.read_exact(&mut buf4)?;
        let num_cols = u32::from_le_bytes(buf4) as usize;

        let mut column_stats = Vec::with_capacity(num_cols);
        for _ in 0..num_cols {
            cursor.read_exact(&mut buf4)?;
            let min_len = u32::from_le_bytes(buf4) as usize;
            let mut min_buf = vec![0u8; min_len];
            if min_len > 0 {
                cursor.read_exact(&mut min_buf)?;
            }
            let min = if min_len > 0 {
                Some(String::from_utf8_lossy(&min_buf).into_owned())
            } else {
                None
            };

            cursor.read_exact(&mut buf4)?;
            let max_len = u32::from_le_bytes(buf4) as usize;
            let mut max_buf = vec![0u8; max_len];
            if max_len > 0 {
                cursor.read_exact(&mut max_buf)?;
            }
            let max = if max_len > 0 {
                Some(String::from_utf8_lossy(&max_buf).into_owned())
            } else {
                None
            };

            cursor.read_exact(&mut buf8)?;
            let null_count = u64::from_le_bytes(buf8);

            column_stats.push(ColumnStats {
                min,
                max,
                null_count,
            });
        }

        Ok(Self {
            row_count,
            column_stats,
        })
    }
}

/// Read the binary row-group metadata footer from any `Read + Seek` source.
pub fn read_row_group_metadata_from_reader<R: Read + Seek>(
    mut reader: R,
) -> std::io::Result<RowGroupMetadata> {
    let file_len = reader.seek(std::io::SeekFrom::End(0))?;
    if file_len < 4 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "file too small for row-group metadata",
        ));
    }

    reader.seek(std::io::SeekFrom::End(-4))?;
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as u64;

    if file_len < 4 + len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid row-group metadata length",
        ));
    }

    reader.seek(std::io::SeekFrom::End(-(4 + len as i64)))?;
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf)?;
    RowGroupMetadata::from_bytes(&buf)
}
