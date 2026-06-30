/* kore.h — KORE Engine C API
 * Auto-generated from kore-ffi/src/lib.rs
 * Link against: libkore_ffi.so  (Linux)
 *               kore_ffi.dll    (Windows)
 *               libkore_ffi.dylib (macOS)
 */
#ifndef KORE_H
#define KORE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque handles ──────────────────────────────────────────────────────── */
typedef struct KoreBlock KoreBlock;
typedef struct KoreModel KoreModel;

/* ── Error handling ─────────────────────────────────────────────────────── */
/** Returns last error message on this thread, or NULL if none. */
const char* kore_last_error(void);

/* ── DataBlock ──────────────────────────────────────────────────────────── */
KoreBlock*  kore_block_new(void);
void        kore_block_free(KoreBlock* block);
uint64_t    kore_block_num_rows(const KoreBlock* block);
uint32_t    kore_block_num_cols(const KoreBlock* block);

/** Add f64 column. NaN values become NULL. Returns 0 on success. */
int kore_block_add_f64(KoreBlock* block, const char* name,
                        const double* data, uint64_t len);

/** Add i64 column. INT64_MIN values become NULL. Returns 0 on success. */
int kore_block_add_i64(KoreBlock* block, const char* name,
                        const int64_t* data, uint64_t len);

/** Read f64 column into out[0..maxlen]. Returns values written, or -1 on error. */
int64_t kore_block_get_f64(const KoreBlock* block, const char* col,
                            double* out, uint64_t maxlen);

/* ── HashJoin ───────────────────────────────────────────────────────────── */
/** join_type: 0=INNER  1=LEFT  2=FULL.
 *  Returns new block (caller must free) or NULL on error. */
KoreBlock* kore_hash_join(const KoreBlock* left, const KoreBlock* right,
                           const char* left_key, const char* right_key,
                           int join_type);

/* ── ML Models ──────────────────────────────────────────────────────────── */
/**
 * model_type:
 *   0 = RandomForestRegressor   (param1=n_trees, param2=max_depth)
 *   1 = RandomForestClassifier  (param1=n_trees, param2=max_depth)
 *   2 = GradientBoostingReg     (param1=n_iters, param2=max_depth)
 *   3 = LinearRegressor         (no params)
 *   4 = LogisticRegressor       (param1=epochs)
 *   5 = KNN Regressor           (param1=k)
 *   6 = KNN Classifier          (param1=k)
 *   7 = LinearSVM               (param1=epochs)
 */
KoreModel* kore_model_new(int model_type, int param1, int param2);
void       kore_model_free(KoreModel* model);

/** Fit model on x_flat (row-major, n_rows × n_cols) and y (n_rows). */
int     kore_model_fit(KoreModel* model, const double* x_flat,
                        uint64_t n_rows, uint64_t n_cols, const double* y);
/** Predict. out must hold n_rows doubles. Returns 0 on success. */
int     kore_model_predict(KoreModel* model, const double* x_flat,
                            uint64_t n_rows, uint64_t n_cols, double* out);

/* ══════════════════════════════════════════════════════════════════════════
 * SQL SESSION API  —  High-level query interface
 * One session = one in-memory database.
 * Supported by all language bindings (Python, Java, Node.js, Go, C#, R, Ruby).
 * ══════════════════════════════════════════════════════════════════════════ */
typedef struct KoreSession KoreSession;

/** Create a new SQL session.  Must be freed with kore_session_free. */
KoreSession* kore_session_new(void);

/** Free a session. */
void kore_session_free(KoreSession* sess);

/** Load a CSV file as a named table.  Returns 0 on success. */
int kore_session_load_csv(KoreSession* sess,
                           const char* table_name, const char* path);

/** Register an existing DataBlock as a named table (copies data). */
int kore_session_register_block(KoreSession* sess,
                                 const char* table_name,
                                 const KoreBlock* block);

/**
 * Execute a SQL query.
 * Returns a heap-allocated JSON string (UTF-8, null-terminated).
 * Caller MUST free the string with kore_free_string().
 * Returns NULL on error — check kore_last_error().
 *
 * Example result: [{"id":1,"name":"Alice","score":95.5}, ...]
 */
char* kore_session_query(KoreSession* sess, const char* sql);

/** Return row count of a table, or -1 if not found. */
int64_t kore_session_row_count(const KoreSession* sess, const char* table_name);

/** Free a string returned by kore_session_query. */
void kore_free_string(char* s);

#ifdef __cplusplus
}
#endif

#endif /* KORE_H */

int kore_model_fit(KoreModel* model,
                   const double* x_flat, uint64_t n_rows, uint64_t n_cols,
                   const double* y);

/** Predict; writes n_rows values to out. */
int kore_model_predict(const KoreModel* model,
                        const double* x_flat, uint64_t n_rows, uint64_t n_cols,
                        double* out);

#ifdef __cplusplus
} /* extern "C" */
#endif
#endif /* KORE_H */
