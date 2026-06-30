// kore.go -- Go bindings for the KORE engine using CGo.
//
// Covers:
//   * DataBlock / ML API  (Block, Model)
//   * SQL Session API     (Session)
//
// Build requirements:
//   1. cargo build --release -p kore-ffi
//   2. export CGO_LDFLAGS="-L/path/to/kore/target/release -lkore_ffi -Wl,-rpath,/path/to/kore/target/release"
//      export CGO_CFLAGS="-I/path/to/kore/kore-ffi/include"
//      go build ./...
//
// Quick usage:
//   sess, _ := kore.NewSession()
//   defer sess.Close()
//   sess.LoadCSV("sales", "/data/sales.csv")
//   rows, _ := sess.Query("SELECT region, SUM(amount) FROM sales GROUP BY region")
//   fmt.Println(rows)

package kore

/*
#cgo LDFLAGS: -lkore_ffi
#include "kore.h"
#include <stdlib.h>
*/
import "C"
import (
"encoding/json"
"errors"
"fmt"
"runtime"
"unsafe"
)

// ============================================================================
// Error handling
// ============================================================================

func lastError() error {
msg := C.kore_last_error()
if msg == nil {
return nil
}
return errors.New(C.GoString(msg))
}

func check(rc C.int) error {
if rc != 0 {
if err := lastError(); err != nil {
return err
}
return fmt.Errorf("kore returned %d", rc)
}
return nil
}

// ============================================================================
// KoreBlock
// ============================================================================

// Block is an in-memory columnar DataBlock.
type Block struct {
ptr unsafe.Pointer
}

// NewBlock allocates a new empty Block.
func NewBlock() *Block {
b := &Block{ptr: unsafe.Pointer(C.kore_block_new())}
runtime.SetFinalizer(b, (*Block).Free)
return b
}

// Free releases the block.  Called automatically by GC if not called manually.
func (b *Block) Free() {
if b.ptr != nil {
C.kore_block_free((*C.KoreBlock)(b.ptr))
b.ptr = nil
}
}

// NumRows returns the number of rows in the block.
func (b *Block) NumRows() uint64 { return uint64(C.kore_block_num_rows((*C.KoreBlock)(b.ptr))) }

// NumCols returns the number of columns in the block.
func (b *Block) NumCols() uint32 { return uint32(C.kore_block_num_cols((*C.KoreBlock)(b.ptr))) }

// AddF64 appends a float64 column.  Use math.NaN() for NULL values.
func (b *Block) AddF64(name string, data []float64) error {
if len(data) == 0 {
return nil
}
cname := C.CString(name)
defer C.free(unsafe.Pointer(cname))
cdata := (*C.double)(unsafe.Pointer(&data[0]))
return check(C.kore_block_add_f64((*C.KoreBlock)(b.ptr), cname, cdata, C.uint64_t(len(data))))
}

// AddI64 appends an int64 column.  Use math.MinInt64 for NULL values.
func (b *Block) AddI64(name string, data []int64) error {
if len(data) == 0 {
return nil
}
cname := C.CString(name)
defer C.free(unsafe.Pointer(cname))
cdata := (*C.longlong)(unsafe.Pointer(&data[0]))
return check(C.kore_block_add_i64((*C.KoreBlock)(b.ptr), cname, cdata, C.uint64_t(len(data))))
}

// GetF64 reads a float64 column into a Go slice.
func (b *Block) GetF64(col string) ([]float64, error) {
n := b.NumRows()
if n == 0 {
return nil, nil
}
out := make([]float64, n)
cname := C.CString(col)
defer C.free(unsafe.Pointer(cname))
cout := (*C.double)(unsafe.Pointer(&out[0]))
rc := int64(C.kore_block_get_f64((*C.KoreBlock)(b.ptr), cname, cout, C.uint64_t(n)))
if rc < 0 {
return nil, lastError()
}
return out[:rc], nil
}

// JoinType specifies the hash-join variant.
type JoinType int

const (
JoinInner JoinType = 0
JoinLeft  JoinType = 1
JoinFull  JoinType = 2
)

// HashJoin performs a hash-join between b (left) and right.
func (b *Block) HashJoin(right *Block, leftKey, rightKey string, how JoinType) (*Block, error) {
lk := C.CString(leftKey)
rk := C.CString(rightKey)
defer C.free(unsafe.Pointer(lk))
defer C.free(unsafe.Pointer(rk))
ptr := C.kore_hash_join(
(*C.KoreBlock)(b.ptr), (*C.KoreBlock)(right.ptr),
lk, rk, C.int(how),
)
if ptr == nil {
return nil, lastError()
}
result := &Block{ptr: unsafe.Pointer(ptr)}
runtime.SetFinalizer(result, (*Block).Free)
return result, nil
}

// ============================================================================
// ML Models
// ============================================================================

// ModelType identifies the ML algorithm.
type ModelType int

const (
RFRegressor     ModelType = 0
RFClassifier    ModelType = 1
GBMRegressor    ModelType = 2
LinearRegressor ModelType = 3
Logistic        ModelType = 4
KNNRegressor    ModelType = 5
KNNClassifier   ModelType = 6
SVM             ModelType = 7
)

// Model wraps a KORE ML model.
type Model struct {
ptr unsafe.Pointer
}

// NewModel allocates a new ML model.
//
//   param1: e.g. n_trees for RF, n_iters for GBM, k for KNN
//   param2: e.g. max_depth
func NewModel(mtype ModelType, param1, param2 int) (*Model, error) {
ptr := C.kore_model_new(C.int(mtype), C.int(param1), C.int(param2))
if ptr == nil {
return nil, lastError()
}
m := &Model{ptr: unsafe.Pointer(ptr)}
runtime.SetFinalizer(m, (*Model).Free)
return m, nil
}

// Free releases the model.
func (m *Model) Free() {
if m.ptr != nil {
C.kore_model_free((*C.KoreModel)(m.ptr))
m.ptr = nil
}
}

// Fit trains the model.  xFlat is a row-major []float64 of length nRows*nCols.
func (m *Model) Fit(xFlat []float64, nRows, nCols int, y []float64) error {
if len(xFlat) == 0 || len(y) == 0 {
return errors.New("xFlat and y must not be empty")
}
xPtr := (*C.double)(unsafe.Pointer(&xFlat[0]))
yPtr := (*C.double)(unsafe.Pointer(&y[0]))
return check(C.kore_model_fit(
(*C.KoreModel)(m.ptr), xPtr,
C.uint64_t(nRows), C.uint64_t(nCols), yPtr,
))
}

// Predict returns predictions for xFlat (row-major, nRows x nCols).
func (m *Model) Predict(xFlat []float64, nRows, nCols int) ([]float64, error) {
if len(xFlat) == 0 {
return nil, errors.New("xFlat must not be empty")
}
xPtr := (*C.double)(unsafe.Pointer(&xFlat[0]))
out  := make([]float64, nRows)
oPtr := (*C.double)(unsafe.Pointer(&out[0]))
if err := check(C.kore_model_predict(
(*C.KoreModel)(m.ptr), xPtr,
C.uint64_t(nRows), C.uint64_t(nCols), oPtr,
)); err != nil {
return nil, err
}
return out, nil
}

// ============================================================================
// SQL Session
// ============================================================================

// Session is a KORE in-memory SQL database.  Create one with NewSession();
// use LoadCSV / LoadBlock / Query / RowCount to interact with it; call Close
// when done (or rely on the finalizer).
type Session struct {
ptr *C.KoreSession
}

// NewSession allocates a new SQL session.
func NewSession() (*Session, error) {
ptr := C.kore_session_new()
if ptr == nil {
if err := lastError(); err != nil {
return nil, err
}
return nil, errors.New("kore_session_new returned NULL")
}
s := &Session{ptr: ptr}
runtime.SetFinalizer(s, (*Session).Close)
return s, nil
}

// Close frees the session.  Safe to call multiple times.
func (s *Session) Close() {
if s.ptr != nil {
C.kore_session_free(s.ptr)
s.ptr = nil
}
}

// LoadCSV registers a CSV file on disk as a named table.
func (s *Session) LoadCSV(table, csvPath string) error {
ct := C.CString(table)
cp := C.CString(csvPath)
defer C.free(unsafe.Pointer(ct))
defer C.free(unsafe.Pointer(cp))
return check(C.kore_session_load_csv(s.ptr, ct, cp))
}

// RegisterBlock copies a Block into the session as a named table.
func (s *Session) RegisterBlock(table string, b *Block) error {
ct := C.CString(table)
defer C.free(unsafe.Pointer(ct))
return check(C.kore_session_register_block(s.ptr, ct, (*C.KoreBlock)(b.ptr)))
}

// Query executes a SQL statement and returns results as a slice of maps.
func (s *Session) Query(sql string) ([]map[string]interface{}, error) {
csql := C.CString(sql)
defer C.free(unsafe.Pointer(csql))

rawPtr := C.kore_session_query(s.ptr, csql)
if rawPtr == nil {
if err := lastError(); err != nil {
return nil, err
}
return nil, errors.New("kore_session_query returned NULL")
}
jsonStr := C.GoString(rawPtr)
C.kore_free_string(rawPtr)

var result []map[string]interface{}
if err := json.Unmarshal([]byte(jsonStr), &result); err != nil {
return nil, fmt.Errorf("JSON decode error: %w", err)
}
return result, nil
}

// RowCount returns the number of rows in a named table, or -1 if not found.
func (s *Session) RowCount(table string) (int64, error) {
ct := C.CString(table)
defer C.free(unsafe.Pointer(ct))
n := int64(C.kore_session_row_count(s.ptr, ct))
if n < 0 {
if err := lastError(); err != nil {
return -1, err
}
return -1, fmt.Errorf("table %q not found", table)
}
return n, nil
}