package kore

import (
	"testing"
)

// ─────────────────────────────────────────────────────────────────────────────
// DATA TYPES
// ─────────────────────────────────────────────────────────────────────────────

func TestDataTypeValues(t *testing.T) {
	tests := []struct {
		name     string
		dataType DataType
		expected uint8
	}{
		{"I64", I64, 1},
		{"F64", F64, 2},
		{"BOOL", BOOL, 3},
		{"STR", STR, 4},
		{"STR_DICT", STR_DICT, 5},
		{"ARRAY", ARRAY, 6},
		{"STRUCT", STRUCT, 7},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if uint8(tt.dataType) != tt.expected {
				t.Errorf("got %d, want %d", tt.dataType, tt.expected)
			}
		})
	}
}

func TestCompressionValues(t *testing.T) {
	tests := []struct {
		name        string
		compression Compression
		expected    uint8
	}{
		{"RAW", RAW, 0},
		{"RLE", RLE, 1},
		{"DELTA", DELTA, 2},
		{"DICT", DICT, 3},
		{"NAN_RAW", NAN_RAW, 4},
		{"DEFLATE", DEFLATE, 5},
		{"ZSTD", ZSTD, 6},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if uint8(tt.compression) != tt.expected {
				t.Errorf("got %d, want %d", tt.compression, tt.expected)
			}
		})
	}
}

func TestDataTypeString(t *testing.T) {
	tests := []struct {
		dataType DataType
		expected string
	}{
		{I64, "I64"},
		{F64, "F64"},
		{BOOL, "BOOL"},
		{STR, "STR"},
		{STR_DICT, "STR_DICT"},
		{ARRAY, "ARRAY"},
		{STRUCT, "STRUCT"},
	}

	for _, tt := range tests {
		t.Run(tt.expected, func(t *testing.T) {
			if tt.dataType.String() != tt.expected {
				t.Errorf("got %s, want %s", tt.dataType.String(), tt.expected)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// DATA BLOCK
// ─────────────────────────────────────────────────────────────────────────────

func TestNewDataBlock(t *testing.T) {
	db := NewDataBlock()

	if db == nil {
		t.Fatal("NewDataBlock returned nil")
	}
	if db.NumRows != 0 {
		t.Errorf("got NumRows %d, want 0", db.NumRows)
	}
	if db.NumColumns() != 0 {
		t.Errorf("got NumColumns %d, want 0", db.NumColumns())
	}
}

func TestAddColumn(t *testing.T) {
	db := NewDataBlock()

	// Add first column
	data := []int64{1, 2, 3, 4, 5}
	if err := db.AddColumn("numbers", I64, data); err != nil {
		t.Fatalf("AddColumn failed: %v", err)
	}

	if db.NumRows != 5 {
		t.Errorf("got NumRows %d, want 5", db.NumRows)
	}
	if db.NumColumns() != 1 {
		t.Errorf("got NumColumns %d, want 1", db.NumColumns())
	}

	// Add second column with same row count
	names := []string{"a", "b", "c", "d", "e"}
	if err := db.AddColumn("names", STR, names); err != nil {
		t.Fatalf("AddColumn failed: %v", err)
	}

	if db.NumColumns() != 2 {
		t.Errorf("got NumColumns %d, want 2", db.NumColumns())
	}
}

func TestAddColumnMismatchedRows(t *testing.T) {
	db := NewDataBlock()

	// Add first column with 5 rows
	if err := db.AddColumn("numbers", I64, []int64{1, 2, 3, 4, 5}); err != nil {
		t.Fatalf("AddColumn failed: %v", err)
	}

	// Try to add column with 3 rows (should fail)
	if err := db.AddColumn("names", STR, []string{"a", "b", "c"}); err == nil {
		t.Error("AddColumn should have failed with mismatched row count")
	}
}

func TestGetColumn(t *testing.T) {
	db := NewDataBlock()

	data := []int64{10, 20, 30}
	if err := db.AddColumn("test", I64, data); err != nil {
		t.Fatalf("AddColumn failed: %v", err)
	}

	col := db.GetColumn("test")
	if col == nil {
		t.Fatal("GetColumn returned nil")
	}

	if col.Name != "test" {
		t.Errorf("got Name %q, want test", col.Name)
	}
	if col.Type != I64 {
		t.Errorf("got Type %d, want %d", col.Type, I64)
	}
}

func TestGetColumnNotFound(t *testing.T) {
	db := NewDataBlock()
	db.AddColumn("test", I64, []int64{1, 2, 3})

	col := db.GetColumn("nonexistent")
	if col != nil {
		t.Errorf("GetColumn should return nil for nonexistent column, got %v", col)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// COLUMN STATS
// ─────────────────────────────────────────────────────────────────────────────

func TestColumnStats(t *testing.T) {
	minVal := 1.0
	maxVal := 100.0

	stats := &ColumnStats{
		MinValue:    &minVal,
		MaxValue:    &maxVal,
		NullCount:   0,
		Cardinality: 50,
		CRC32:       0xdeadbeef,
	}

	if *stats.MinValue != 1.0 {
		t.Errorf("got MinValue %f, want 1.0", *stats.MinValue)
	}
	if *stats.MaxValue != 100.0 {
		t.Errorf("got MaxValue %f, want 100.0", *stats.MaxValue)
	}
	if stats.NullCount != 0 {
		t.Errorf("got NullCount %d, want 0", stats.NullCount)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// CRC32
// ─────────────────────────────────────────────────────────────────────────────

func TestCRC32(t *testing.T) {
	// Test with simple data
	data := []byte{1, 2, 3, 4, 5}
	crc := CRC32(data)

	if crc == 0 {
		t.Error("CRC32 returned 0")
	}

	// CRC32 of same data should be consistent
	crc2 := CRC32(data)
	if crc != crc2 {
		t.Errorf("CRC32 not consistent: got %d and %d", crc, crc2)
	}
}

func TestCRC32Empty(t *testing.T) {
	crc := CRC32([]byte{})
	if crc != 0 {
		t.Errorf("CRC32 of empty slice should be 0, got %d", crc)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// VERSION
// ─────────────────────────────────────────────────────────────────────────────

func TestVersion(t *testing.T) {
	if Version != "2.0.0" {
		t.Errorf("got Version %s, want 2.0.0", Version)
	}
}
