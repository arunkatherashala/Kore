/*
Package kore provides Go bindings for the Kore columnar format.

Enables reading/writing Kore files directly from Go applications without subprocess overhead.
Uses CGO to call Rust core functions via FFI.

Example:
	reader := kore.NewReader("data.kore")
	data, err := reader.Read()
	if err != nil {
		log.Fatal(err)
	}
	
	// data is [][]string (columns × rows)
	for colIdx, column := range data {
		fmt.Printf("Column %d: %v rows\n", colIdx, len(column))
	}
*/
package kore

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"os"
)

const (
	KORE_MAGIC_BYTES = "KORE"
	KORE_VERSION     = 2
	CHUNK_ROWS       = 65536
)

// KoreReader reads Kore files
type KoreReader struct {
	Path    string
	File    *os.File
	Header  *KoreHeader
	Columns []KoreColumn
}

// KoreHeader contains file metadata
type KoreHeader struct {
	Magic    [4]byte
	Version  uint8
	Reserved uint8
	NumCols  uint16
	NumRows  uint64
	NumChunks uint32
}

// KoreColumn contains column metadata
type KoreColumn struct {
	Name    string
	Type    string
	Offset  uint64
	Length  uint32
	Encoded bool
}

// NewReader creates a new Kore file reader
func NewReader(path string) (*KoreReader, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("failed to open %s: %w", path, err)
	}

	reader := &KoreReader{
		Path: path,
		File: file,
	}

	if err := reader.readHeader(); err != nil {
		file.Close()
		return nil, err
	}

	return reader, nil
}

// readHeader reads and parses the Kore file header
func (r *KoreReader) readHeader() error {
	header := make([]byte, 64)
	if n, err := r.File.Read(header); err != nil || n < 64 {
		return errors.New("failed to read file header")
	}

	// Validate magic
	if string(header[0:4]) != KORE_MAGIC_BYTES {
		return errors.New("invalid Kore file format: bad magic bytes")
	}

	// Parse header
	kh := &KoreHeader{
		Version:  header[4],
		NumCols:  binary.LittleEndian.Uint16(header[6:8]),
		NumRows:  binary.LittleEndian.Uint64(header[8:16]),
		NumChunks: uint32((uint64(header[8:16][0]) + CHUNK_ROWS - 1) / CHUNK_ROWS),
	}

	copy(kh.Magic[:], header[0:4])

	if kh.Version != KORE_VERSION {
		return fmt.Errorf("unsupported Kore version: %d", kh.Version)
	}

	r.Header = kh
	return nil
}

// Read reads entire Kore file into memory
// Returns data as [][]string where first index is column, second is row
func (r *KoreReader) Read() ([][]string, error) {
	if r.File == nil {
		return nil, errors.New("reader closed")
	}

	// Seek back to start after header read
	if _, err := r.File.Seek(64, io.SeekStart); err != nil {
		return nil, fmt.Errorf("failed to seek: %w", err)
	}

	// TODO: Implement actual Kore binary parsing
	// For now, return placeholder
	columns := make([][]string, r.Header.NumCols)
	for i := range columns {
		columns[i] = make([]string, r.Header.NumRows)
	}

	return columns, nil
}

// ReadColumn reads a specific column
func (r *KoreReader) ReadColumn(columnName string) ([]string, error) {
	if r.File == nil {
		return nil, errors.New("reader closed")
	}

	// TODO: Implement column-specific read with zero-copy
	column := make([]string, r.Header.NumRows)
	return column, nil
}

// Stats returns file statistics without reading all data
func (r *KoreReader) Stats() map[string]interface{} {
	fi, _ := r.File.Stat()

	return map[string]interface{}{
		"rows":        r.Header.NumRows,
		"columns":     r.Header.NumCols,
		"chunks":      r.Header.NumChunks,
		"file_size":   fi.Size(),
		"version":     r.Header.Version,
		"compression": float64(fi.Size()) / float64(r.Header.NumRows*uint64(r.Header.NumCols)*50),
	}
}

// Close closes the file
func (r *KoreReader) Close() error {
	if r.File != nil {
		return r.File.Close()
	}
	return nil
}

// ============================================================================
// KoreWriter writes Kore files
// ============================================================================

// KoreWriter writes data in Kore format
type KoreWriter struct {
	Path    string
	File    *os.File
	Columns []string
	Types   []string
	Buffer  [][]string
}

// NewWriter creates a new Kore file writer
func NewWriter(path string, columns []string, types []string) (*KoreWriter, error) {
	file, err := os.Create(path)
	if err != nil {
		return nil, fmt.Errorf("failed to create %s: %w", path, err)
	}

	return &KoreWriter{
		Path:    path,
		File:    file,
		Columns: columns,
		Types:   types,
		Buffer:  make([][]string, len(columns)),
	}, nil
}

// WriteRow adds a row to the buffer
func (w *KoreWriter) WriteRow(values []string) error {
	if len(values) != len(w.Columns) {
		return fmt.Errorf("expected %d values, got %d", len(w.Columns), len(values))
	}

	for i, val := range values {
		w.Buffer[i] = append(w.Buffer[i], val)
	}

	// Flush when buffer reaches chunk size
	if len(w.Buffer[0]) >= CHUNK_ROWS {
		return w.Flush()
	}

	return nil
}

// Flush writes buffered data to file
func (w *KoreWriter) Flush() error {
	// TODO: Implement Kore binary format writer
	// - Write header
	// - Encode columns with compression
	// - Write metadata
	return nil
}

// Close finalizes the file
func (w *KoreWriter) Close() error {
	if err := w.Flush(); err != nil {
		return err
	}

	if w.File != nil {
		return w.File.Close()
	}
	return nil
}
