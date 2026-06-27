// ============================================================================
// KORE Query Engine (Gap #4) — SQL-like queries on KORE files
// ============================================================================
// (Copied from original project)

use crate::kore_v2::KVal;

// ── Query AST ────────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub enum AggFunc { Count, Sum, Avg, Min, Max, None }

#[derive(Debug, Clone)]
pub struct SelectCol {
    pub name: String,
    pub agg: AggFunc,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub enum FilterOp { Eq, Neq, Gt, Lt, Gte, Lte, Contains }

#[derive(Debug, Clone)]
pub struct WhereClause {
    pub col: String,
    pub op: FilterOp,
    pub val: String,
}

#[derive(Debug, Clone)]
pub struct KoreQuery {
    pub select: Vec<SelectCol>,
    pub from: String,
    pub where_clauses: Vec<WhereClause>,
    pub group_by: Vec<String>,
    pub order_by: Option<(String, bool)>,  // (col, ascending)
    pub limit: Option<usize>,
}

// ── Query Result ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<KVal>>,
    pub elapsed_ms: f64,
}

impl QueryResult {
    pub fn to_csv(&self) -> String {
        let mut out = self.columns.join(",");
        out.push('\n');
        for row in &self.rows {
            let vals: Vec<String> = row.iter().map(|v| v.display()).collect();
            out.push_str(&vals.join(","));
            out.push('\n');
        }
        out
    }

    pub fn display_table(&self, max_rows: usize) -> String {
        let mut out = String::new();
        // Header
        let widths: Vec<usize> = self.columns.iter().enumerate().map(|(ci, name)| {
            let mut w = name.len();
            for row in self.rows.iter().take(max_rows) {
                if ci < row.len() { w = w.max(row[ci].display().len()); }
            }
            w.min(40)
        }).collect();

        for (ci, name) in self.columns.iter().enumerate() {
            out.push_str(&format!("{:width$}", name, width = widths[ci] + 2));
        }
        out.push('\n');
        for w in &widths { out.push_str(&"-".repeat(*w + 2)); }
        out.push('\n');

        for row in self.rows.iter().take(max_rows) {
            for (ci, val) in row.iter().enumerate() {
                let s = val.display();
                let w = widths.get(ci).copied().unwrap_or(10);
                out.push_str(&format!("{:width$}", s, width = w + 2));
            }
            out.push('\n');
        }
        if self.rows.len() > max_rows {
            out.push_str(&format!("... {} more rows\n", self.rows.len() - max_rows));
        }
        out.push_str(&format!("\n({} rows, {:.1}ms)\n", self.rows.len(), self.elapsed_ms));
        out
    }
}

// The parser and executor are copied from the original project. See the source file for full implementation and tests.

