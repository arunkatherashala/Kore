#![allow(dead_code, unused_variables, unused_mut)]

// ============================================================================
// KORE Transactions (Gap #7 + #8) — Time Travel + ACID Writes
// ============================================================================
// (Copied from original project)

#[derive(Debug, Clone)]
pub struct KoreVersion {
    pub version: u64,
    pub timestamp: u64,
    pub message: String,
    pub filename: String,
    pub nrows: usize,
    pub size_bytes: u64,
}

#[derive(Debug)]
pub struct KoreVersionLog {
    pub base_path: String,
    pub versions: Vec<KoreVersion>,
}

impl KoreVersionLog {
    pub fn open(kore_path: &str) -> Self {
        let manifest_path = format!("{}.versions", kore_path);
        let versions = if let Ok(contents) = std::fs::read_to_string(&manifest_path) {
            contents.lines().filter_map(|line| {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 6 {
                    Some(KoreVersion {
                        version: parts[0].parse().ok()?,
                        timestamp: parts[1].parse().ok()?,
                        message: parts[2].to_string(),
                        filename: parts[3].to_string(),
                        nrows: parts[4].parse().ok()?,
                        size_bytes: parts[5].parse().ok()?,
                    })
                } else { None }
            }).collect()
        } else { Vec::new() };
        KoreVersionLog { base_path: kore_path.to_string(), versions }
    }

    pub fn latest_version(&self) -> u64 { self.versions.last().map(|v| v.version).unwrap_or(0) }

    pub fn record_version(&mut self, message: &str, filename: &str, nrows: usize, size_bytes: u64) -> Result<u64, String> {
        let version = self.latest_version() + 1;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        self.versions.push(KoreVersion {
            version,
            timestamp: ts,
            message: message.to_string(),
            filename: filename.to_string(),
            nrows,
            size_bytes,
        });
        self.save()?;
        Ok(version)
    }

    fn save(&self) -> Result<(), String> {
        let manifest_path = format!("{}.versions", self.base_path);
        let mut out = String::new();
        for v in &self.versions {
            out.push_str(&format!(
                "{}|{}|{}|{}|{}|{}\n",
                v.version,
                v.timestamp,
                v.message.replace('|', "/"),
                v.filename,
                v.nrows,
                v.size_bytes
            ));
        }
        std::fs::write(&manifest_path, out)
            .map_err(|e| format!("Cannot write {}: {}", manifest_path, e))
    }
}

pub struct KoreTxn {
    pub base_path: String,
    temp_path: String,
    committed: bool,
}

impl KoreTxn {
    pub fn begin(kore_path: &str) -> Self {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let temp_path = format!("{}.tmp.{}.kore", kore_path, ts);
        KoreTxn { base_path: kore_path.to_string(), temp_path, committed: false }
    }

    pub fn temp_path(&self) -> &str { &self.temp_path }

    pub fn commit(mut self, message: &str) -> Result<u64, String> {
        if self.committed {
            return Err("Transaction already finalized".to_string());
        }
        if !std::path::Path::new(&self.temp_path).exists() {
            return Err("No staged temp file found; write data to temp_path() before commit".to_string());
        }
        std::fs::rename(&self.temp_path, &self.base_path)
            .map_err(|e| format!("Cannot promote temp file: {}", e))?;
        self.committed = true;

        let mut log = KoreVersionLog::open(&self.base_path);
        let nrows = 0usize;
        let sz = std::fs::metadata(&self.base_path).map(|m| m.len()).unwrap_or(0);
        log.record_version(message, &self.base_path, nrows, sz)
    }

    pub fn abort(mut self) {
        let _ = std::fs::remove_file(&self.temp_path);
        self.committed = true;
    }
}

pub fn checkout(_kore_path: &str, _version: u64) -> Result<(), String> {
    Err("checkout is not implemented in this standalone crate yet".to_string())
}

pub fn as_of(_kore_path: &str, _timestamp: u64) -> Result<(), String> {
    Err("as_of is not implemented in this standalone crate yet".to_string())
}

pub fn list_versions(kore_path: &str) -> Vec<String> {
    let log = KoreVersionLog::open(kore_path);
    log.versions
        .iter()
        .map(|v| format!("v{} {} {}", v.version, v.timestamp, v.message))
        .collect()
}

