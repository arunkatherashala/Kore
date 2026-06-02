package kore

import (
	"os"
	"testing"
)

func TestNewReaderInvalidFile(t *testing.T) {
	_, err := NewReader("/nonexistent/file.kore")
	if err == nil {
		t.Error("Expected error for nonexistent file, got nil")
	}
}

func TestWriteAndReadRoundtrip(t *testing.T) {
	// Create temporary file
	tmpfile, err := os.CreateTemp("", "kore-*.kore")
	if err != nil {
		t.Fatalf("Failed to create temp file: %v", err)
	}
	defer os.Remove(tmpfile.Name())
	tmpfile.Close()

	// Test data
	testData := [][]string{
		{"alice", "bob", "charlie"},
		{"engineer", "designer", "manager"},
		{"25", "30", "35"},
	}

	// Write data
	err = WriteFile(tmpfile.Name(), testData)
	if err != nil {
		t.Fatalf("WriteFile failed: %v", err)
	}

	// Read data back
	readData, err := ReadFile(tmpfile.Name())
	if err != nil {
		t.Fatalf("ReadFile failed: %v", err)
	}

	// Verify
	if len(readData) != len(testData) {
		t.Errorf("Expected %d columns, got %d", len(testData), len(readData))
	}

	for colIdx, column := range readData {
		if len(column) != len(testData[colIdx]) {
			t.Errorf("Column %d: expected %d rows, got %d", colIdx, len(testData[colIdx]), len(column))
		}

		for rowIdx, value := range column {
			if value != testData[colIdx][rowIdx] {
				t.Errorf("Cell [%d,%d]: expected %q, got %q", colIdx, rowIdx, testData[colIdx][rowIdx], value)
			}
		}
	}
}

func TestReadColumn(t *testing.T) {
	tmpfile, err := os.CreateTemp("", "kore-*.kore")
	if err != nil {
		t.Fatalf("Failed to create temp file: %v", err)
	}
	defer os.Remove(tmpfile.Name())
	tmpfile.Close()

	testData := [][]string{
		{"a", "b", "c"},
		{"1", "2", "3"},
	}

	WriteFile(tmpfile.Name(), testData)

	reader, err := NewReader(tmpfile.Name())
	if err != nil {
		t.Fatalf("NewReader failed: %v", err)
	}
	defer reader.Close()

	column, err := reader.ReadColumn(0)
	if err != nil {
		t.Fatalf("ReadColumn failed: %v", err)
	}

	if len(column) != len(testData[0]) {
		t.Errorf("Expected %d rows, got %d", len(testData[0]), len(column))
	}

	for i, val := range column {
		if val != testData[0][i] {
			t.Errorf("Row %d: expected %q, got %q", i, testData[0][i], val)
		}
	}
}

func TestHeader(t *testing.T) {
	tmpfile, err := os.CreateTemp("", "kore-*.kore")
	if err != nil {
		t.Fatalf("Failed to create temp file: %v", err)
	}
	defer os.Remove(tmpfile.Name())
	tmpfile.Close()

	testData := [][]string{
		{"a", "b"},
		{"1", "2"},
		{"x", "y"},
	}

	WriteFile(tmpfile.Name(), testData)

	reader, err := NewReader(tmpfile.Name())
	if err != nil {
		t.Fatalf("NewReader failed: %v", err)
	}
	defer reader.Close()

	header := reader.Header()
	if header.NumCols != 3 {
		t.Errorf("Expected 3 columns, got %d", header.NumCols)
	}
	if header.NumRows != 2 {
		t.Errorf("Expected 2 rows, got %d", header.NumRows)
	}
}

func TestVersion(t *testing.T) {
	v := Version()
	if v != "1.2.2" {
		t.Errorf("Expected version 1.2.2, got %s", v)
	}
}

func BenchmarkWrite(b *testing.B) {
	tmpfile, _ := os.CreateTemp("", "kore-*.kore")
	defer os.Remove(tmpfile.Name())
	tmpfile.Close()

	// Generate large test data
	cols := 10
	rows := 10000
	testData := make([][]string, cols)
	for i := 0; i < cols; i++ {
		testData[i] = make([]string, rows)
		for j := 0; j < rows; j++ {
			testData[i][j] = "test_value"
		}
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		WriteFile(tmpfile.Name(), testData)
	}
}

func BenchmarkRead(b *testing.B) {
	tmpfile, _ := os.CreateTemp("", "kore-*.kore")
	defer os.Remove(tmpfile.Name())
	tmpfile.Close()

	// Setup test data
	cols := 10
	rows := 10000
	testData := make([][]string, cols)
	for i := 0; i < cols; i++ {
		testData[i] = make([]string, rows)
		for j := 0; j < rows; j++ {
			testData[i][j] = "test_value"
		}
	}
	WriteFile(tmpfile.Name(), testData)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		ReadFile(tmpfile.Name())
	}
}
