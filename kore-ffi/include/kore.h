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

/* ── File I/O ─────────────────────────────────────────────────────────── */

/** Write a block to a .kore file. Returns 0 on success, -1 on error. */
int kore_write_file(const char* path, const KoreBlock* block);

/** Read a .kore file. Returns NULL on error. Free with kore_block_free. */
KoreBlock* kore_read_file(const char* path);

/** Serialise a block to bytes. Free the result with kore_free_bytes. */
uint8_t* kore_write_bytes(const KoreBlock* block, size_t* out_len);

/** Parse bytes into a block. Returns NULL on corrupt data (kore_last_error has details). */
KoreBlock* kore_read_bytes(const uint8_t* data, size_t len);

/** Column projection: decode only the named columns (in that order); others are skipped without
 *  decompression. Returns NULL on error, including an unknown column name. */
KoreBlock* kore_read_bytes_columns(const uint8_t* data, size_t len,
                                   const char* const* names, size_t n_names);

void kore_free_bytes(uint8_t* ptr, size_t len);

uint32_t kore_crc32(const uint8_t* data, size_t len);

/** Column name by index. Free with kore_free_string. */
char* kore_block_col_name(const KoreBlock* block, size_t idx);

/** Column type by index: 0=int64 1=float64 2=bool 3=string 4=string-dict, -1 if out of range. */
int kore_block_col_type(const KoreBlock* block, size_t idx);

int64_t kore_block_get_i64(const KoreBlock* block, const char* col,
                           long long* out, uint64_t maxlen);

/* ── Typed columns with explicit nulls (NaN stays NaN) ────────────────── */
/* `validity`: one byte per row (1 = valid, 0 = null), or NULL if every row is valid. */

int kore_block_add_f64_v(KoreBlock* block, const char* name, const double* data,
                         const uint8_t* validity, uint64_t len);
int kore_block_add_i64_v(KoreBlock* block, const char* name, const long long* data,
                         const uint8_t* validity, uint64_t len);
/** `data`: one byte (0/1) per row. */
int kore_block_add_bool_v(KoreBlock* block, const char* name, const uint8_t* data,
                          const uint8_t* validity, uint64_t len);
/** `offsets`: len + 1 byte offsets into UTF-8 `bytes`. Returns -1 on invalid UTF-8 or bad offsets. */
int kore_block_add_str_v(KoreBlock* block, const char* name, const uint32_t* offsets,
                         const uint8_t* bytes, uint64_t bytes_len,
                         const uint8_t* validity, uint64_t len);
/** Dictionary strings: `codes` one byte per row (255 = null), at most 254 dictionary entries
 *  given as dict_len + 1 offsets into UTF-8 `dict_bytes`. */
int kore_block_add_strdict_v(KoreBlock* block, const char* name, const uint8_t* codes, uint64_t len,
                             const uint32_t* dict_offsets, const uint8_t* dict_bytes,
                             uint64_t dict_bytes_len, uint64_t dict_len);

/** Column getters by index; return the row count or -1. Null slots hold NaN (f64) or 0. */
int64_t kore_block_get_f64_v(const KoreBlock* block, size_t idx, double* out,
                             uint8_t* validity_out, uint64_t maxlen);
int64_t kore_block_get_i64_v(const KoreBlock* block, size_t idx, long long* out,
                             uint8_t* validity_out, uint64_t maxlen);
int64_t kore_block_get_bool_v(const KoreBlock* block, size_t idx, uint8_t* out,
                              uint8_t* validity_out, uint64_t maxlen);
/** Total UTF-8 bytes of a string column, or -1 if it is not one. */
int64_t kore_block_str_bytes(const KoreBlock* block, size_t idx);
/** offsets_out needs rows + 1 entries, bytes_out kore_block_str_bytes() bytes. */
int64_t kore_block_get_str_v(const KoreBlock* block, size_t idx, uint32_t* offsets_out,
                             uint8_t* bytes_out, uint64_t bytes_cap, uint8_t* validity_out);
/** Returns 1 (and sizes) for a dictionary string column, 0 for others, -1 if out of range. */
int kore_block_strdict_info(const KoreBlock* block, size_t idx, uint64_t* dict_len_out,
                            uint64_t* dict_bytes_out);
int64_t kore_block_get_strdict_v(const KoreBlock* block, size_t idx, uint8_t* codes_out,
                                 uint32_t* dict_offsets_out, uint8_t* dict_bytes_out);

/* ── Encryption (AES-256-GCM, PBKDF2-HMAC-SHA256 key) ─────────────────── */

/** Returns NULL on error. Free the result with kore_free_bytes. */
uint8_t* kore_encrypt_bytes(const uint8_t* password, size_t pw_len,
                            const uint8_t* data, size_t len, size_t* out_len);

/** Returns NULL on wrong password or corrupt data. Free with kore_free_bytes. */
uint8_t* kore_decrypt_bytes(const uint8_t* password, size_t pw_len,
                            const uint8_t* data, size_t len, size_t* out_len);

/* ── Time travel (append-only version log) ────────────────────────────── */

/**
 * Append `entry` (a complete .kore file) as a new version. `existing` may be NULL
 * (new log), a version log, or a plain .kore file (becomes version 0).
 * `timestamp` must exceed the latest version's. Free with kore_free_bytes.
 */
uint8_t* kore_version_append(const uint8_t* existing, size_t existing_len,
                             const uint8_t* entry, size_t entry_len,
                             uint64_t timestamp, size_t* out_len);

/** Newest version with timestamp <= target, or NULL if none. Free with kore_free_bytes. */
uint8_t* kore_version_select(const uint8_t* data, size_t len,
                             uint64_t target, size_t* out_len);

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
