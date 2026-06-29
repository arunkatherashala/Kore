// kore.go — Go bindings for the KORE engine using CGo.
//
// Build requirements:
//   1. cargo build --release -p kore-ffi
//   2. Set CGO_LDFLAGS and CGO_CFLAGS to point at the built library:
//
//      export CGO_LDFLAGS="-L/path/to/kore/target/release -lkore_ffi -Wl,-rpath,/path/to/kore/target/release"
//      export CGO_CFLAGS="-I/path/to/kore/kore-ffi/include"
//      go build ./...
//
// Usage:
//   engine := kore.New()
//   defer engine.Close()
//   block := engine.NewBlock()
//   defer block.Free()
//   block.AddF64("score", []float64{1.0, 2.0, 3.0})

package kore

/*
#cgo LDFLAGS: -lkore_ffi
#include "kore.h"
#include <stdlib.h>
*/
import "C"
import (
	"errors"
	"fmt"
	"runtime"
	"unsafe"
)

// ── Error handling ────────────────────────────────────────────────────────────

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

// ── KoreBlock ─────────────────────────────────────────────────────────────────

type Block struct {
	ptr unsafe.Pointer
}

func NewBlock() *Block {
	b := &Block{ptr: C.kore_block_new()}
	runtime.SetFinalizer(b, (*Block).Free)
	return b
}

func (b *Block) Free() {
	if b.ptr != nil {
		C.kore_block_free(b.ptr)
		b.ptr = nil
	}
}

func (b *Block) NumRows() uint64 { return uint64(C.kore_block_num_rows(b.ptr)) }
func (b *Block) NumCols() uint32 { return uint32(C.kore_block_num_cols(b.ptr)) }

func (b *Block) AddF64(name string, data []float64) error {
	cname := C.CString(name)
	defer C.free(unsafe.Pointer(cname))
	cdata := (*C.double)(unsafe.Pointer(&data[0]))
	return check(C.kore_block_add_f64(b.ptr, cname, cdata, C.uint64_t(len(data))))
}

func (b *Block) AddI64(name string, data []int64) error {
	cname := C.CString(name)
	defer C.free(unsafe.Pointer(cname))
	cdata := (*C.longlong)(unsafe.Pointer(&data[0]))
	return check(C.kore_block_add_i64(b.ptr, cname, cdata, C.uint64_t(len(data))))
}

func (b *Block) GetF64(col string) ([]float64, error) {
	n := b.NumRows()
	out := make([]float64, n)
	cname := C.CString(col)
	defer C.free(unsafe.Pointer(cname))
	cout := (*C.double)(unsafe.Pointer(&out[0]))
	rc := int64(C.kore_block_get_f64(b.ptr, cname, cout, C.uint64_t(n)))
	if rc < 0 {
		return nil, lastError()
	}
	return out[:rc], nil
}

// ── JoinType ──────────────────────────────────────────────────────────────────

type JoinType int

const (
	JoinInner JoinType = 0
	JoinLeft  JoinType = 1
	JoinFull  JoinType = 2
)

func (b *Block) HashJoin(right *Block, leftKey, rightKey string, how JoinType) (*Block, error) {
	lk := C.CString(leftKey)
	rk := C.CString(rightKey)
	defer C.free(unsafe.Pointer(lk))
	defer C.free(unsafe.Pointer(rk))
	ptr := C.kore_hash_join(b.ptr, right.ptr, lk, rk, C.int(how))
	if ptr == nil {
		return nil, lastError()
	}
	result := &Block{ptr: ptr}
	runtime.SetFinalizer(result, (*Block).Free)
	return result, nil
}

// ── ModelType ─────────────────────────────────────────────────────────────────

type ModelType int

const (
	RFRegressor    ModelType = 0
	RFClassifier   ModelType = 1
	GBMRegressor   ModelType = 2
	LinearRegressor ModelType = 3
	Logistic        ModelType = 4
	KNNRegressor    ModelType = 5
	KNNClassifier   ModelType = 6
	SVM             ModelType = 7
)

// ── KoreModel ─────────────────────────────────────────────────────────────────

type Model struct {
	ptr unsafe.Pointer
}

func NewModel(mtype ModelType, param1, param2 int) (*Model, error) {
	ptr := C.kore_model_new(C.int(mtype), C.int(param1), C.int(param2))
	if ptr == nil {
		return nil, lastError()
	}
	m := &Model{ptr: ptr}
	runtime.SetFinalizer(m, (*Model).Free)
	return m, nil
}

func (m *Model) Free() {
	if m.ptr != nil {
		C.kore_model_free(m.ptr)
		m.ptr = nil
	}
}

// Fit trains the model.  xFlat is a row-major []float64 of length nRows*nCols.
func (m *Model) Fit(xFlat []float64, nRows, nCols int, y []float64) error {
	xPtr := (*C.double)(unsafe.Pointer(&xFlat[0]))
	yPtr := (*C.double)(unsafe.Pointer(&y[0]))
	return check(C.kore_model_fit(m.ptr, xPtr, C.uint64_t(nRows), C.uint64_t(nCols), yPtr))
}

// Predict returns predictions for xFlat (row-major, nRows×nCols).
func (m *Model) Predict(xFlat []float64, nRows, nCols int) ([]float64, error) {
	xPtr := (*C.double)(unsafe.Pointer(&xFlat[0]))
	out  := make([]float64, nRows)
	oPtr := (*C.double)(unsafe.Pointer(&out[0]))
	if err := check(C.kore_model_predict(m.ptr, xPtr, C.uint64_t(nRows), C.uint64_t(nCols), oPtr)); err != nil {
		return nil, err
	}
	return out, nil
}
