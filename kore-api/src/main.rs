//! KORE Layer 25 — REST + WebSocket API server.
//!
//! Endpoints:
//!   GET  /health                    → {"status":"ok","version":"1.0","layers":25}
//!   POST /api/v1/query              → KQL query on in-memory context
//!   POST /api/v1/tables/{name}      → Register a DataBlock from JSON
//!   GET  /api/v1/tables             → List registered tables
//!   POST /api/v1/ml/fit             → Train a model
//!   POST /api/v1/ml/predict/{id}    → Predict with a trained model
//!   GET  /api/v1/ml/models          → List models
//!   DELETE /api/v1/ml/models/{id}   → Delete a model

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

use kore_core::{Column, ColumnData, DataBlock};
use kore_ml2::{GradientBoostingRegressor, RandomForestClassifier, RandomForestRegressor};
use kore_ml3::{KNearestNeighbors, LinearRegressor, LinearSVM, LogisticRegressor};
use kore_sql::KqlContext;

mod model_registry;
use model_registry::{ModelEntry, ModelRegistry};

// ─── Shared state ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct AppState {
    context:  Mutex<KqlContext>,
    models:   Mutex<ModelRegistry>,
}

type SharedState = Arc<AppState>;

// ─── main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let port = std::env::var("KORE_PORT").unwrap_or_else(|_| "8080".into());
    let addr = format!("0.0.0.0:{port}");

    let state: SharedState = Arc::new(AppState::default());

    let app = Router::new()
        // health
        .route("/health", get(health))
        // tables
        .route("/api/v1/tables",       get(list_tables))
        .route("/api/v1/tables/{name}", post(register_table))
        // KQL query
        .route("/api/v1/query",        post(run_query))
        // ML
        .route("/api/v1/ml/models",          get(list_models))
        .route("/api/v1/ml/fit",             post(fit_model))
        .route("/api/v1/ml/predict/{id}",    post(predict_model))
        .route("/api/v1/ml/models/{id}",     delete(delete_model))
        // CORS
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("KORE API listening on http://{addr}");
    println!("Layers: 15=Join  16=Cache  17=ML2  18=Pipeline  19=Cluster  20=Bench");
    println!("        21=SQL   22=Store  23=ML3  24=FFI       25=API (this server)");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ─── Health ───────────────────────────────────────────────────────────────────

async fn health() -> Json<Value> {
    Json(json!({
        "status":  "ok",
        "version": "1.0",
        "engine":  "KORE",
        "layers":  25,
        "algorithms": ["HashJoin","BroadcastJoin","SortMergeJoin","LRU","MatView",
                        "RandomForest","GBM","NaiveBayes","DecisionTree",
                        "Pipeline","Cluster","KQL","ColumnarStore",
                        "LinearRegression","KNN","SVM","LogisticRegression"]
    }))
}

// ─── Table registration ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TableJson {
    columns: Vec<ColumnJson>,
}

#[derive(Deserialize)]
struct ColumnJson {
    name:   String,
    #[serde(rename = "type")]
    dtype:  String,    // "f64" | "i64" | "bool" | "str"
    values: Value,
}

async fn register_table(
    State(state): State<SharedState>,
    Path(name):   Path<String>,
    Json(body):   Json<TableJson>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mut cols:     Vec<Column> = Vec::new();
    let mut num_rows: usize       = 0;

    for cj in body.columns {
        let (data, n) = parse_column_json(&cj)?;
        num_rows = n;
        cols.push(Column { name: cj.name, data });
    }

    let block = DataBlock { columns: cols, num_rows };
    state.context.lock().unwrap().register(name.clone(), block);

    Ok(Json(json!({ "registered": name, "rows": num_rows })))
}

fn parse_column_json(cj: &ColumnJson) -> Result<(ColumnData, usize), (StatusCode, String)> {
    let arr = cj.values.as_array()
        .ok_or((StatusCode::BAD_REQUEST, "values must be an array".into()))?;
    let n = arr.len();
    Ok(match cj.dtype.as_str() {
        "f64" => {
            let v: Vec<Option<f64>> = arr.iter().map(|x| x.as_f64()).collect();
            (ColumnData::Float64(v), n)
        }
        "i64" => {
            let v: Vec<Option<i64>> = arr.iter().map(|x| x.as_i64()).collect();
            (ColumnData::Int64(v), n)
        }
        "bool" => {
            let v: Vec<Option<bool>> = arr.iter().map(|x| x.as_bool()).collect();
            (ColumnData::Bool(v), n)
        }
        "str" => {
            let v: Vec<Option<String>> = arr.iter().map(|x| x.as_str().map(|s| s.to_string())).collect();
            (ColumnData::Str(v), n)
        }
        other => return Err((StatusCode::BAD_REQUEST, format!("unknown dtype: {other}"))),
    })
}

async fn list_tables(State(state): State<SharedState>) -> Json<Value> {
    let ctx   = state.context.lock().unwrap();
    let names: Vec<String> = ctx.table_names();
    Json(json!({ "tables": names }))
}

// ─── KQL query ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct QueryRequest { sql: String }

async fn run_query(
    State(state): State<SharedState>,
    Json(req):    Json<QueryRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let ctx    = state.context.lock().unwrap();
    let result = ctx.query(&req.sql)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let cols: Vec<Value> = result.columns.iter().map(|c| {
        let values: Value = match &c.data {
            ColumnData::Float64(v) => json!(v.iter().map(|x| x.map(|f| f)).collect::<Vec<_>>()),
            ColumnData::Int64(v)   => json!(v.iter().map(|x| x.map(|i| i)).collect::<Vec<_>>()),
            ColumnData::Bool(v)    => json!(v.iter().map(|x| x.map(|b| b)).collect::<Vec<_>>()),
            ColumnData::Str(v)     => json!(v.iter().map(|x| x.as_deref()).collect::<Vec<_>>()),
        };
        json!({ "name": c.name, "values": values })
    }).collect();

    Ok(Json(json!({ "rows": result.num_rows, "columns": cols })))
}

// ─── ML fit ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FitRequest {
    model_type: String,   // "rf_reg"|"rf_clf"|"gbm"|"linreg"|"knn_reg"|"knn_clf"|"svm"|"logistic"
    param1:     Option<usize>,
    param2:     Option<usize>,
    x:          Vec<Vec<f64>>,
    y:          Vec<f64>,
}

async fn fit_model(
    State(state): State<SharedState>,
    Json(req):    Json<FitRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let p1 = req.param1.unwrap_or(100);
    let p2 = req.param2.unwrap_or(3);
    let x  = req.x;
    let y  = req.y;

    let entry = match req.model_type.as_str() {
        "rf_reg" => {
            let mut m = RandomForestRegressor::new(p1, p2);
            m.fit_raw(&x, &y);
            ModelEntry::RfReg(m)
        }
        "rf_clf" => {
            let mut m = RandomForestClassifier::new(p1, p2);
            m.fit_raw(&x, &y);
            ModelEntry::RfClf(m)
        }
        "gbm" => {
            let mut m = GradientBoostingRegressor::new(p1, 0.1, p2);
            m.fit_raw(&x, &y);
            ModelEntry::Gbm(m)
        }
        "linreg" => {
            let mut m = LinearRegressor::new(1e-8);
            m.fit_raw(&x, &y);
            ModelEntry::LinReg(m)
        }
        "logistic" => {
            let mut m = LogisticRegressor::new(0.1, p1, 32, 1e-4);
            m.fit_raw(&x, &y);
            ModelEntry::Logistic(m)
        }
        "knn_reg" => {
            let mut m = KNearestNeighbors::new_regressor(p1);
            m.fit_raw(&x, &y);
            ModelEntry::KnnReg(m)
        }
        "knn_clf" => {
            let mut m = KNearestNeighbors::new_classifier(p1);
            m.fit_raw(&x, &y);
            ModelEntry::KnnClf(m)
        }
        "svm" => {
            let mut m = LinearSVM::new(0.01, p1);
            m.fit_raw(&x, &y);
            ModelEntry::Svm(m)
        }
        other => return Err((StatusCode::BAD_REQUEST, format!("unknown model_type: {other}"))),
    };

    let id = state.models.lock().unwrap().insert(req.model_type.clone(), entry);
    Ok(Json(json!({ "model_id": id, "model_type": req.model_type })))
}

// ─── ML predict ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PredictRequest { x: Vec<Vec<f64>> }

async fn predict_model(
    State(state): State<SharedState>,
    Path(id):     Path<usize>,
    Json(req):    Json<PredictRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let models = state.models.lock().unwrap();
    let preds  = models.predict(id, &req.x)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(json!({ "model_id": id, "predictions": preds })))
}

async fn list_models(State(state): State<SharedState>) -> Json<Value> {
    let models = state.models.lock().unwrap();
    Json(json!({ "models": models.list() }))
}

async fn delete_model(
    State(state): State<SharedState>,
    Path(id):     Path<usize>,
) -> Result<Json<Value>, (StatusCode, String)> {
    state.models.lock().unwrap().remove(id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(json!({ "deleted": id })))
}
