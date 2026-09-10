use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::PyDict;

use kore_core::{DataBlock, Value};
use kore_sql::KqlContext;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn strip_qualifier(name: &str) -> &str {
    name.rfind('.').map(|i| &name[i + 1..]).unwrap_or(name)
}

fn value_to_py(py: Python<'_>, val: Value) -> PyObject {
    match val {
        Value::Int(i) => i.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Float(f) => f.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Bool(b) => (&*b.into_pyobject(py).unwrap()).clone().into_any().unbind(),
        Value::Str(s) => s.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Array(a) => format!("{:?}", a).into_pyobject(py).unwrap().into_any().unbind(),
        Value::Map(m) => format!("{:?}", m).into_pyobject(py).unwrap().into_any().unbind(),
        Value::Null => py.None(),
    }
}

fn datablock_to_py_dicts(py: Python<'_>, block: &DataBlock) -> PyResult<Vec<PyObject>> {
    let mut rows = Vec::with_capacity(block.num_rows);
    for r in 0..block.num_rows {
        let dict = PyDict::new(py);
        for col in &block.columns {
            let key = strip_qualifier(&col.name);
            let val = value_to_py(py, col.data.get_value(r));
            dict.set_item(key, val)?;
        }
        rows.push(dict.into_any().unbind());
    }
    Ok(rows)
}

fn format_table(block: &DataBlock) -> String {
    if block.columns.is_empty() {
        return "(empty)".to_string();
    }

    let headers: Vec<String> = block
        .columns
        .iter()
        .map(|c| strip_qualifier(&c.name).to_string())
        .collect();

    let mut col_data: Vec<Vec<String>> = Vec::new();
    for col in &block.columns {
        let mut vals = Vec::with_capacity(block.num_rows);
        for r in 0..block.num_rows {
            vals.push(match col.data.get_value(r) {
                Value::Int(i) => i.to_string(),
                Value::Float(f) => format!("{f:.2}"),
                Value::Bool(b) => b.to_string(),
                Value::Str(s) => s,
                Value::Array(a) => format!("{:?}", a),
                Value::Map(m) => format!("{:?}", m),
                Value::Null => "NULL".to_string(),
            });
        }
        col_data.push(vals);
    }

    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(ci, h)| {
            let max_data = col_data[ci].iter().map(|v| v.len()).max().unwrap_or(0);
            h.len().max(max_data)
        })
        .collect();

    let mut out = String::new();
    let sep: String = widths
        .iter()
        .map(|w| "-".repeat(*w + 2))
        .collect::<Vec<_>>()
        .join("+");

    out.push('+');
    out.push_str(&sep);
    out.push_str("+\n");

    out.push('|');
    for (i, h) in headers.iter().enumerate() {
        out.push_str(&format!(" {:>width$} |", h, width = widths[i]));
    }
    out.push('\n');

    out.push('+');
    out.push_str(&sep);
    out.push_str("+\n");

    for r in 0..block.num_rows {
        out.push('|');
        for ci in 0..headers.len() {
            out.push_str(&format!(
                " {:>width$} |",
                col_data[ci][r],
                width = widths[ci]
            ));
        }
        out.push('\n');
    }

    out.push('+');
    out.push_str(&sep);
    out.push_str("+\n");
    out
}

// ─── DataFrame operations ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum DfOp {
    Filter(String),
    Select(Vec<String>),
    GroupBy(Vec<String>),
    Agg(Vec<(String, String)>),
    Join { other_table: String, on: String },
    OrderBy(String, bool),
    Limit(usize),
}

fn build_sql(table_name: &str, operations: &[DfOp]) -> String {
    let mut select_clause = String::from("*");
    let mut from_clause = table_name.to_string();
    let mut where_parts: Vec<String> = Vec::new();
    let mut group_parts: Vec<String> = Vec::new();
    let mut order_parts: Vec<String> = Vec::new();
    let mut limit_clause: Option<usize> = None;

    for op in operations {
        match op {
            DfOp::Select(cols) => {
                select_clause = cols.join(", ");
            }
            DfOp::Filter(pred) => {
                where_parts.push(pred.clone());
            }
            DfOp::GroupBy(keys) => {
                group_parts = keys.clone();
            }
            DfOp::Agg(pairs) => {
                let agg_exprs: Vec<String> = pairs
                    .iter()
                    .map(|(col, func)| {
                        format!(
                            "{func}({col}) AS {f}_{col}",
                            f = func.to_lowercase()
                        )
                    })
                    .collect();
                if group_parts.is_empty() {
                    select_clause = agg_exprs.join(", ");
                } else {
                    select_clause =
                        format!("{}, {}", group_parts.join(", "), agg_exprs.join(", "));
                }
            }
            DfOp::Join { other_table, on } => {
                from_clause = format!(
                    "{from_clause} INNER JOIN {other_table} ON {table_name}.{on} = {other_table}.{on}"
                );
            }
            DfOp::OrderBy(col, desc) => {
                let dir = if *desc { "DESC" } else { "ASC" };
                order_parts.push(format!("{col} {dir}"));
            }
            DfOp::Limit(n) => {
                limit_clause = Some(*n);
            }
        }
    }

    let mut sql = format!("SELECT {select_clause} FROM {from_clause}");
    if !where_parts.is_empty() {
        sql.push_str(&format!(" WHERE {}", where_parts.join(" AND ")));
    }
    if !group_parts.is_empty() {
        sql.push_str(&format!(" GROUP BY {}", group_parts.join(", ")));
    }
    if !order_parts.is_empty() {
        sql.push_str(&format!(" ORDER BY {}", order_parts.join(", ")));
    }
    if let Some(n) = limit_clause {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    sql
}

// ─── KoreSession ─────────────────────────────────────────────────────────────

#[pyclass]
struct KoreSession {
    ctx: KqlContext,
}

#[pymethods]
impl KoreSession {
    #[new]
    fn new() -> Self {
        Self {
            ctx: KqlContext::new(),
        }
    }

    fn load_csv(&mut self, path: &str, table_name: &str) -> PyResult<()> {
        let block = kore_io::CsvReader::new(path)
            .read()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))?;
        self.ctx.register(table_name, block);
        Ok(())
    }

    fn read_parquet(&mut self, path: &str, table_name: &str) -> PyResult<()> {
        let block = kore_parquet::ParquetReader::new(path)
            .read()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))?;
        self.ctx.register(table_name, block);
        Ok(())
    }

    fn sql(&self, py: Python<'_>, query: &str) -> PyResult<Vec<PyObject>> {
        let block = self
            .ctx
            .query(query)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
        datablock_to_py_dicts(py, &block)
    }

    fn table(slf: &Bound<'_, Self>, name: &str) -> PyResult<KoreDataFrame> {
        {
            let session = slf.borrow();
            if session.ctx.get(name).is_none() {
                return Err(PyErr::new::<pyo3::exceptions::PyKeyError, _>(format!(
                    "Table not found: {name}"
                )));
            }
        }
        Ok(KoreDataFrame {
            session: slf.clone().unbind(),
            table_name: name.to_string(),
            operations: Vec::new(),
        })
    }

    fn write_parquet(&self, query: &str, path: &str) -> PyResult<()> {
        let block = self
            .ctx
            .query(query)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
        kore_parquet::ParquetWriter::write_file(&block, path)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))?;
        Ok(())
    }

    fn tables(&self) -> Vec<String> {
        self.ctx.table_names()
    }
}

// ─── KoreDataFrame ───────────────────────────────────────────────────────────

#[pyclass]
struct KoreDataFrame {
    session: Py<KoreSession>,
    table_name: String,
    operations: Vec<DfOp>,
}

impl KoreDataFrame {
    fn with_op(&self, py: Python<'_>, op: DfOp) -> Self {
        let mut ops = self.operations.clone();
        ops.push(op);
        Self {
            session: self.session.clone_ref(py),
            table_name: self.table_name.clone(),
            operations: ops,
        }
    }

    fn execute(&self, py: Python<'_>) -> PyResult<DataBlock> {
        let sql = build_sql(&self.table_name, &self.operations);
        let session = self.session.borrow(py);
        session
            .ctx
            .query(&sql)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    }
}

#[pymethods]
impl KoreDataFrame {
    fn filter(&self, py: Python<'_>, predicate: &str) -> Self {
        self.with_op(py, DfOp::Filter(predicate.to_string()))
    }

    fn select(&self, py: Python<'_>, cols: Vec<String>) -> Self {
        self.with_op(py, DfOp::Select(cols))
    }

    #[pyo3(name = "groupBy")]
    fn group_by(&self, py: Python<'_>, keys: Vec<String>) -> Self {
        self.with_op(py, DfOp::GroupBy(keys))
    }

    fn agg(&self, py: Python<'_>, aggregations: HashMap<String, String>) -> Self {
        let pairs: Vec<(String, String)> = aggregations.into_iter().collect();
        self.with_op(py, DfOp::Agg(pairs))
    }

    fn join(&self, py: Python<'_>, other: &KoreDataFrame, on: &str) -> Self {
        self.with_op(py, DfOp::Join {
            other_table: other.table_name.clone(),
            on: on.to_string(),
        })
    }

    #[pyo3(name = "orderBy")]
    #[pyo3(signature = (col, desc=None))]
    fn order_by(&self, py: Python<'_>, col: &str, desc: Option<bool>) -> Self {
        self.with_op(py, DfOp::OrderBy(col.to_string(), desc.unwrap_or(false)))
    }

    fn limit(&self, py: Python<'_>, n: usize) -> Self {
        self.with_op(py, DfOp::Limit(n))
    }

    fn show(&self, py: Python<'_>) -> PyResult<()> {
        let block = self.execute(py)?;
        print!("{}", format_table(&block));
        Ok(())
    }

    fn collect(&self, py: Python<'_>) -> PyResult<Vec<PyObject>> {
        let block = self.execute(py)?;
        datablock_to_py_dicts(py, &block)
    }

    fn count(&self, py: Python<'_>) -> PyResult<usize> {
        let block = self.execute(py)?;
        Ok(block.num_rows)
    }

    fn to_pandas(&self, py: Python<'_>) -> PyResult<PyObject> {
        let block = self.execute(py)?;
        let pandas = py.import("pandas")?;
        let dict = PyDict::new(py);
        for col in &block.columns {
            let key = strip_qualifier(&col.name);
            let values: Vec<PyObject> = (0..block.num_rows)
                .map(|r| value_to_py(py, col.data.get_value(r)))
                .collect();
            dict.set_item(key, values)?;
        }
        let df = pandas.call_method1("DataFrame", (dict,))?;
        Ok(df.into_any().unbind())
    }
}

// ─── Module ──────────────────────────────────────────────────────────────────

#[pymodule]
fn kore_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<KoreSession>()?;
    m.add_class::<KoreDataFrame>()?;
    Ok(())
}
