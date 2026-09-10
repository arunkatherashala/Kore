//! Arrow IPC transport helpers for KORE network layer.
//!
//! Provides DataBlock <-> Arrow IPC bytes conversion for the shuffle path.
//! Uses kore-arrow's compact format (50% smaller than MsgPack-serialized DataBlock).

use kore_core::DataBlock;
use kore_arrow::{ArrowBlock, ipc_encode, ipc_decode};

/// Encode a DataBlock into Arrow IPC bytes for network transfer.
/// Returns compact binary representation (validity bitmaps, no Option overhead).
pub fn arrow_encode_block(block: &DataBlock) -> Vec<u8> {
    let ab = ArrowBlock::from_data_block(block);
    ipc_encode(&ab)
}

/// Decode Arrow IPC bytes back into a DataBlock.
/// Returns None if the bytes are invalid.
pub fn arrow_decode_block(bytes: &[u8]) -> Option<DataBlock> {
    ipc_decode(bytes).ok().map(|ab| ab.to_data_block())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData};

    #[test]
    fn roundtrip_arrow_transport() {
        let block = DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(), data: ColumnData::Int64(vec![Some(1), Some(2), Some(3)]) },
                Column { name: "val".into(), data: ColumnData::Float64(vec![Some(1.5), None, Some(3.5)]) },
                Column { name: "name".into(), data: ColumnData::Str(vec![Some("a".into()), Some("b".into()), None]) },
            ],
        };

        let bytes = arrow_encode_block(&block);
        let decoded = arrow_decode_block(&bytes).expect("decode");
        assert_eq!(decoded.num_rows, 3);
        assert_eq!(decoded.columns.len(), 3);
    }

    #[test]
    fn arrow_transport_smaller_than_json() {
        let n = 1000;
        let block = DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "x".into(), data: ColumnData::Float64((0..n).map(|i| Some(i as f64 * 1.1)).collect()) },
                Column { name: "y".into(), data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
            ],
        };

        let arrow_bytes = arrow_encode_block(&block);
        let json_bytes = serde_json::to_vec(&block).unwrap();
        assert!(arrow_bytes.len() < json_bytes.len(),
            "Arrow IPC ({} bytes) should be smaller than JSON ({} bytes)",
            arrow_bytes.len(), json_bytes.len());
    }
}
