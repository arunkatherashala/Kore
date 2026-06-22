use std::fs::File;
use std::io::{self, BufRead, BufReader};

/// Simple streaming decoder prototype: reads a file line-by-line and invokes the
/// provided callback for each line. Designed to avoid materializing whole columns.
pub fn stream_lines<P: AsRef<std::path::Path>, F: FnMut(String)>(path: P, mut cb: F) -> io::Result<()> {
    let f = File::open(path)?;
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let l = line?;
        cb(l);
    }
    Ok(())
}

/// Convenience: stream and print up to `limit` lines (0 = no limit).
pub fn print_sample(path: &std::path::Path, limit: usize) -> io::Result<()> {
    let mut count = 0usize;
    stream_lines(path, |line| {
        if limit == 0 || count < limit {
            println!("{}", line);
        }
        count += 1;
    })
}
