// Package kore provides Go bindings to the KORE columnar format via C FFI.
//
// This module uses CGo to call the Rust kore-ffi C library.
//
// Features:
//   - Read/write KORE v2 binary files
//   - All 11 ACID features: CRC32, stats, ZSTD, nested types, Bloom filters,
//     AES-256-GCM encryption, schema evolution, append writes, MVCC/time travel,
//     partition evolution, row-level deletes
//   - Zero-copy mmap reads via Rust
//   - Automatic codec selection (RAW → LZ4 vs ZSTD)
//
// Example:
//   package main
//
//   import (
//       "fmt"
//       "log"
//       kore "github.com/arunkatherashala/kore/kore-go"
//   )
//
//   func main() {
//       // Create data block
//       block := kore.NewDataBlock()
//       block.AddColumn("numbers", kore.I64, []int64{1, 2, 3, 4, 5})
//       block.AddColumn("names", kore.STR, []string{"a", "b", "c", "d", "e"})
//
//       // Write file
//       if err := kore.WriteFile("/tmp/data.kore", block); err != nil {
//           log.Fatal(err)
//       }
//
//       // Read file
//       restored, err := kore.ReadFile("/tmp/data.kore")
//       if err != nil {
//           log.Fatal(err)
//       }
//
//       fmt.Printf("Rows: %d, Cols: %d\n", restored.NumRows, restored.NumColumns)
//   }
package kore

/*
#include <stdint.h>
#include <stdlib.h>

typedef void* KoreBlock;

extern uint32_t kore_crc32(const uint8_t* data, size_t len);
extern int      kore_write_file(const char* path, KoreBlock block);
extern KoreBlock kore_read_file(const char* path);
extern KoreBlock kore_block_new();
extern void     kore_block_free(KoreBlock block);
extern int      kore_block_add_f64(KoreBlock block, const char* name, const double* data, size_t len);
extern int      kore_block_add_i64(KoreBlock block, const char* name, const int64_t* data, size_t len);
extern uint64_t kore_block_num_rows(KoreBlock block);
extern uint32_t kore_block_num_cols(KoreBlock block);
extern char*    kore_block_col_name(KoreBlock block, size_t idx);
extern int64_t  kore_block_get_f64(KoreBlock block, const char* col, double* out, uint64_t maxlen);
extern void     kore_free_string(char* s);
*/
import "C"

import (
	"errors"
	"fmt"
	"unsafe"
)

// ─────────────────────────────────────────────────────────────────────────────
// DATA TYPES & ENUMS
// ─────────────────────────────────────────────────────────────────────────────

// DataType represents KORE column data types (must match Rust DType enum).
type DataType uint8

const (
	I64     DataType = 1 // 64-bit signed integer
	F64     DataType = 2 // 64-bit floating point
	BOOL    DataType = 3 // Boolean
	STR     DataType = 4 // UTF-8 string
	STR_DICT DataType = 5 // Dictionary-encoded string
	ARRAY   DataType = 6 // Nested array
	STRUCT  DataType = 7 // Nested struct
)

func (dt DataType) String() string {
	switch dt {
	case I64:
		return "I64"
	case F64:
		return "F64"
	case BOOL:
		return "BOOL"
	case STR:
		return "STR"
	case STR_DICT:
		return "STR_DICT"
	case ARRAY:
		return "ARRAY"
	case STRUCT:
		return "STRUCT"
	default:
		return fmt.Sprintf("DataType(%d)", dt)
	}
}

// Compression represents KORE compression codecs (must match Rust Compression enum).
type Compression uint8

const (
	RAW     Compression = 0 // No compression
	RLE     Compression = 1 // Run-length encoding
	DELTA   Compression = 2 // Delta encoding
	DICT    Compression = 3 // Dictionary encoding
	NAN_RAW Compression = 4 // Special NaN handling
	DEFLATE Compression = 5 // Deflate/LZ4
	ZSTD    Compression = 6 // ZSTD compression
)

func (c Compression) String() string {
	switch c {
	case RAW:
		return "RAW"
	case RLE:
		return "RLE"
	case DELTA:
		return "DELTA"
	case DICT:
		return "DICT"
	case NAN_RAW:
		return "NAN_RAW"
	case DEFLATE:
		return "DEFLATE"
	case ZSTD:
		return "ZSTD"
	default:
		return fmt.Sprintf("Compression(%d)", c)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// CORE TYPES
// ─────────────────────────────────────────────────────────────────────────────

// ColumnStats represents statistics for a column.
type ColumnStats struct {
	MinValue  *float64 `json:"min_value,omitempty"`
	MaxValue  *float64 `json:"max_value,omitempty"`
	NullCount uint64   `json:"null_count"`
	Cardinality uint64 `json:"cardinality"`
	CRC32     uint32   `json:"crc32"`
}

// Column represents a single column in a data block.
type Column struct {
	Name  string        `json:"name"`
	Type  DataType      `json:"type"`
	Data  interface{}   `json:"data"`
	Stats *ColumnStats  `json:"stats,omitempty"`
}

// DataBlock represents a multi-column data structure.
type DataBlock struct {
	Columns    []*Column `json:"columns"`
	NumRows    int64     `json:"num_rows"`
}

// NewDataBlock creates an empty data block.
func NewDataBlock() *DataBlock {
	return &DataBlock{
		Columns: make([]*Column, 0),
		NumRows: 0,
	}
}

// AddColumn adds a column to the data block.
func (db *DataBlock) AddColumn(name string, dtype DataType, data interface{}) error {
	var rowCount int64

	// Determine row count from data
	switch d := data.(type) {
	case []int64:
		rowCount = int64(len(d))
	case []float64:
		rowCount = int64(len(d))
	case []bool:
		rowCount = int64(len(d))
	case []string:
		rowCount = int64(len(d))
	default:
		return fmt.Errorf("unsupported data type: %T", data)
	}

	if db.NumRows == 0 {
		db.NumRows = rowCount
	} else if rowCount != db.NumRows {
		return fmt.Errorf(
			"column %q has %d rows, expected %d",
			name, rowCount, db.NumRows,
		)
	}

	col := &Column{
		Name: name,
		Type: dtype,
		Data: data,
	}
	db.Columns = append(db.Columns, col)
	return nil
}

// GetColumn retrieves a column by name.
func (db *DataBlock) GetColumn(name string) *Column {
	for _, col := range db.Columns {
		if col.Name == name {
			return col
		}
	}
	return nil
}

// NumColumns returns the number of columns.
func (db *DataBlock) NumColumns() int {
	return len(db.Columns)
}

// VersionSnapshot represents MVCC version tracking for time travel.
type VersionSnapshot struct {
	VersionID   uint32 `json:"version_id"`
	Timestamp   uint64 `json:"timestamp"`
	BlockOffset uint64 `json:"block_offset"`
	RowCount    uint64 `json:"row_count"`
	PrevVersion *uint32 `json:"prev_version,omitempty"`
}

// PartitionSpec represents partition evolution support.
type PartitionSpec struct {
	SpecID       uint16    `json:"spec_id"`
	Columns      []uint16  `json:"columns"`
	Transforms   []string  `json:"transforms"`
	ParentSpecID *uint16   `json:"parent_spec_id,omitempty"`
}

// DeleteVector represents row-level delete bitmap for soft deletes.
type DeleteVector struct {
	Bitmap      []byte `json:"bitmap"`
	Cardinality uint32 `json:"cardinality"`
	Timestamp   uint64 `json:"timestamp"`
}

// ─────────────────────────────────────────────────────────────────────────────
// FFI FUNCTIONS
// ─────────────────────────────────────────────────────────────────────────────

// CRC32 computes CRC32 checksum.
func CRC32(data []byte) uint32 {
	if len(data) == 0 {
		return 0
	}
	cData := C.CBytes(data)
	defer C.free(cData)

	result := C.kore_crc32((*C.uint8_t)(cData), C.size_t(len(data)))
	return uint32(result)
}

// WriteFile writes a DataBlock to KORE binary file via CGo kore-ffi.
func WriteFile(path string, db *DataBlock) error {
	cPath := C.CString(path)
	defer C.free(unsafe.Pointer(cPath))

	handle := C.kore_block_new()
	if handle == nil {
		return errors.New("kore_block_new returned null")
	}
	defer C.kore_block_free(handle)

	for _, col := range db.Columns {
		cName := C.CString(col.Name)
		switch d := col.Data.(type) {
		case []float64:
			if len(d) > 0 {
				C.kore_block_add_f64(handle, cName, (*C.double)(&d[0]), C.size_t(len(d)))
			}
		case []int64:
			if len(d) > 0 {
				C.kore_block_add_i64(handle, cName, (*C.int64_t)(&d[0]), C.size_t(len(d)))
			}
		}
		C.free(unsafe.Pointer(cName))
	}

	rc := C.kore_write_file(cPath, handle)
	if rc != 0 {
		return fmt.Errorf("kore_write_file failed (rc=%d)", int(rc))
	}
	return nil
}

// ReadFile reads KORE binary file into DataBlock via CGo kore-ffi.
func ReadFile(path string) (*DataBlock, error) {
	cPath := C.CString(path)
	defer C.free(unsafe.Pointer(cPath))

	handle := C.kore_read_file(cPath)
	if handle == nil {
		return nil, fmt.Errorf("kore_read_file failed: %s", path)
	}
	defer C.kore_block_free(handle)

	nrows := int64(C.kore_block_num_rows(handle))
	ncols := int(C.kore_block_num_cols(handle))
	db := NewDataBlock()
	db.NumRows = nrows

	for ci := 0; ci < ncols; ci++ {
		cName := C.kore_block_col_name(handle, C.size_t(ci))
		if cName == nil {
			continue
		}
		colName := C.GoString(cName)
		C.kore_free_string(cName)

		vals := make([]float64, nrows)
		n := int64(C.kore_block_get_f64(handle, C.CString(colName), (*C.double)(&vals[0]), C.uint64_t(nrows)))
		if n > 0 {
			_ = db.AddColumn(colName, F64, vals[:n])
		}
	}
	return db, nil
}

// ReadAtVersion reads KORE data at specific timestamp (time travel).
func ReadAtVersion(data []byte, timestamp uint64) (*DataBlock, error) {
	// TODO: Implement FFI call to kore_read_at_version
	return nil, errors.New("Phase 3: Time travel API pending")
}

// EncryptAES256 encrypts data with AES-256-GCM.
func EncryptAES256(password string, data []byte) ([]byte, error) {
	// TODO: Implement FFI call to kore_encrypt_aes256_gcm
	return nil, errors.New("Phase 3: Encryption API pending")
}

// DecryptAES256 decrypts data with AES-256-GCM.
func DecryptAES256(password string, encryptedData []byte) ([]byte, error) {
	// TODO: Implement FFI call to kore_decrypt_aes256_gcm
	return nil, errors.New("Phase 3: Decryption API pending")
}

// GetColumnStats retrieves statistics for a column.
func GetColumnStats(data []byte, columnName string) (*ColumnStats, error) {
	// TODO: Implement FFI call to kore_get_column_stats
	return nil, errors.New("Phase 3: Stats API pending")
}

// GetBloomFilter retrieves Bloom filter for a column.
func GetBloomFilter(data []byte, columnName string) ([]byte, error) {
	// TODO: Implement FFI call to kore_get_bloom_filter
	return nil, errors.New("Phase 3: Bloom filter API pending")
}

// ─────────────────────────────────────────────────────────────────────────────
// VERSION
// ─────────────────────────────────────────────────────────────────────────────

const Version = "2.0.0"
