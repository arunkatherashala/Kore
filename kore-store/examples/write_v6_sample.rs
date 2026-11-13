//! Input for the old-reader check of rule mode (docs/kore_rule_mode.md): writes into DIR
//! version-6 files (rule mode), a version-4 and a version-5 file of the same block, and corrupted and
//! truncated copies of a version-6 file (`*.kore`, readable by the `read_hash` example of any version).
//!
//!   write_v6_sample DIR

#[path = "support/rules_gen.rs"]
mod gen;

use kore_store::{CompactOptions, KoreWriter, RuleOptions};

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let w = |name: &str, b: &[u8]| std::fs::write(dir.join(name), b).unwrap();
    let orders = gen::orders(2000, 1);
    let v6 = KoreWriter::to_bytes_rules(&orders, &RuleOptions::default()).unwrap();
    w("orders_v6.kore", &v6);
    w("orders_v6_level19.kore", &KoreWriter::to_bytes_rules(&orders, &CompactOptions::default().with_zstd_level(19).with_rules()).unwrap());
    w("sensors_v6.kore", &KoreWriter::to_bytes_rules(&gen::sensors(2000, 2), &RuleOptions::default()).unwrap());
    w("logs_v6.kore", &KoreWriter::to_bytes_rules(&gen::logs(2000, 3), &RuleOptions::default()).unwrap());
    w("orders_v4.kore", &KoreWriter::to_bytes_compact(&orders, &CompactOptions::default()).unwrap());
    w("orders_v5.kore", &KoreWriter::to_bytes_compact(&orders, &CompactOptions::default().with_compact_directory(true)).unwrap());
    let mut flip = v6.clone();
    let k = flip.len() / 2;
    flip[k] ^= 0x40;
    w("orders_v6_flipped.kore", &flip);
    w("orders_v6_truncated.kore", &v6[..v6.len() - 7]);
}
