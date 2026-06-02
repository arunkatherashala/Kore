package kore

import (
	"encoding/binary"
	"errors"
	"fmt"
	"os"
)

const (
	KoreMagic   = "KORE"
	KoreVersion = 2
	ChunkRows   = 65536
)

var (
	ErrInvalidFormat = errors.New("invalid Kore file format")
	ErrInvalidHeader = errors.New("invalid file header")
	ErrReadFailed    = errors.New("failed to read from file")
)

// Header represents the Kore file header
type Header struct {
	Magic    [4]byte
	Version  uint8
	Reserved uint8
	NumCols  uint16
	NumRows  uint64
	NumChunks uint32
}

// Column represents a column in the Kore file
type Column struct {
	Name    string
	Type    string
	Offset  uint64
	Length  uint32
	Encoded bool
}

// Reader reads Kore columnar files
type Reader struct {
	path    string
	file    *os.File
	header  *Header
	columns []Column
}

// Writer writes Kore columnar files
type Writer struct {
	path    string
	file    *os.File
	header  *Header
	columns []Column
}

// NewReader creates a new Kore file reader
func NewReader(path string) (*Reader, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("failed to open file: %w", err)
	}

	reader := &Reader{
		path: path,
		file: file,
	}

	if err := reader.readHeader(); err != nil {
		file.Close()
		return nil, err
	}

	return reader, nil
}

// readHeader reads and validates the Kore file header
func (r *Reader) readHeader() error {
	headerBuf := make([]byte, 64)
	n, err := r.file.Read(headerBuf)
	if err != nil || n < 64 {
		return ErrInvalidHeader
	}

	// Validate magic bytes
	if string(headerBuf[0:4]) != KoreMagic {
		return ErrInvalidFormat
	}

	r.header = &Header{
		Version:  headerBuf[4],
		NumCols:  binary.LittleEndian.Uint16(headerBuf[6:8]),
		NumRows:  binary.LittleEndian.Uint64(headerBuf[8:16]),
		NumChunks: uint32((binary.LittleEndian.Uint64(headerBuf[8:16]) + ChunkRows - 1) / ChunkRows),
	}

	// Read column metadata
	r.columns = make([]Column, r.header.NumCols)
	for i := 0; i < int(r.header.NumCols); i++ {
		colBuf := make([]byte, 256)
		n, err := r.file.Read(colBuf)
		if err != nil || n < 256 {
			return ErrReadFailed
		}

		nameLen := int(binary.LittleEndian.Uint16(colBuf[0:2]))
		r.columns[i].Name = string(colBuf[2 : 2+nameLen])
		r.columns[i].Type = string(colBuf[66:130])
		r.columns[i].Offset = binary.LittleEndian.Uint64(colBuf[130:138])
		r.columns[i].Length = binary.LittleEndian.Uint32(colBuf[138:142])
		r.columns[i].Encoded = colBuf[142] != 0
	}

	return nil
}

// Read reads all data from the Kore file as [][]string (columns of strings)
func (r *Reader) Read() ([][]string, error) {
	if r.header == nil {
		return nil, errors.New("file header not loaded")
	}

	data := make([][]string, r.header.NumCols)
	for i := range data {
		data[i] = make([]string, r.header.NumRows)
	}

	// Read all chunks
	for chunk := 0; chunk < int(r.header.NumChunks); chunk++ {
		chunkSize := uint64(ChunkRows)
		if uint64(chunk)*chunkSize+chunkSize > r.header.NumRows {
			chunkSize = r.header.NumRows - uint64(chunk)*chunkSize
		}

		for col := 0; col < int(r.header.NumCols); col++ {
			for row := 0; row < int(chunkSize); row++ {
				// Read string value from column
				strLen := make([]byte, 4)
				if _, err := r.file.Read(strLen); err != nil {
					return nil, fmt.Errorf("failed to read string length: %w", err)
				}

				len := binary.LittleEndian.Uint32(strLen)
				if len > 0 {
					strBuf := make([]byte, len)
					if _, err := r.file.Read(strBuf); err != nil {
						return nil, fmt.Errorf("failed to read string data: %w", err)
					}
					data[col][uint64(chunk)*uint64(ChunkRows)+uint64(row)] = string(strBuf)
				}
			}
		}
	}

	return data, nil
}

// ReadColumn reads a specific column from the file
func (r *Reader) ReadColumn(colIdx int) ([]string, error) {
	if colIdx >= int(r.header.NumCols) {
		return nil, errors.New("column index out of range")
	}

	col := make([]string, r.header.NumRows)

	for chunk := 0; chunk < int(r.header.NumChunks); chunk++ {
		chunkSize := uint64(ChunkRows)
		if uint64(chunk)*chunkSize+chunkSize > r.header.NumRows {
			chunkSize = r.header.NumRows - uint64(chunk)*chunkSize
		}

		for row := 0; row < int(chunkSize); row++ {
			// Read value
			val := make([]byte, 4)
			if _, err := r.file.Read(val); err != nil {
				return nil, err
			}
			len := binary.LittleEndian.Uint32(val)
			if len > 0 {
				strBuf := make([]byte, len)
				if _, err := r.file.Read(strBuf); err != nil {
					return nil, err
				}
				col[uint64(chunk)*uint64(ChunkRows)+uint64(row)] = string(strBuf)
			}
		}
	}

	return col, nil
}

// Header returns the file header
func (r *Reader) Header() *Header {
	return r.header
}

// Columns returns column metadata
func (r *Reader) Columns() []Column {
	return r.columns
}

// Close closes the reader
func (r *Reader) Close() error {
	if r.file != nil {
		return r.file.Close()
	}
	return nil
}

// NewWriter creates a new Kore file writer
func NewWriter(path string) (*Writer, error) {
	file, err := os.Create(path)
	if err != nil {
		return nil, fmt.Errorf("failed to create file: %w", err)
	}

	return &Writer{
		path: path,
		file: file,
	}, nil
}

// WriteData writes column-oriented data to a Kore file
func (w *Writer) WriteData(columns [][]string) error {
	if len(columns) == 0 {
		return errors.New("no columns to write")
	}

	numRows := uint64(len(columns[0]))
	for _, col := range columns {
		if len(col) != int(numRows) {
			return errors.New("all columns must have the same number of rows")
		}
	}

	// Write header (64 bytes to match reader expectation)
	w.header = &Header{
		Version:   KoreVersion,
		NumCols:   uint16(len(columns)),
		NumRows:   numRows,
		NumChunks: uint32((numRows + ChunkRows - 1) / ChunkRows),
	}

	headerBuf := make([]byte, 64)
	copy(headerBuf[0:4], []byte(KoreMagic))
	headerBuf[4] = w.header.Version
	headerBuf[5] = w.header.Reserved
	binary.LittleEndian.PutUint16(headerBuf[6:8], w.header.NumCols)
	binary.LittleEndian.PutUint64(headerBuf[8:16], w.header.NumRows)

	if _, err := w.file.Write(headerBuf); err != nil {
		return err
	}

	// Write column metadata
	w.columns = make([]Column, len(columns))
	for i := range columns {
		w.columns[i].Name = fmt.Sprintf("col_%d", i)
		w.columns[i].Type = "string"
		w.columns[i].Length = uint32(len(columns[i][0]))
	}

	for _, col := range w.columns {
		// Write 256-byte column metadata entry to match reader expectation
		colBuf := make([]byte, 256)
		binary.LittleEndian.PutUint16(colBuf[0:2], uint16(len(col.Name)))
		copy(colBuf[2:66], []byte(col.Name))
		copy(colBuf[66:130], []byte(col.Type))
		binary.LittleEndian.PutUint64(colBuf[130:138], col.Offset)
		binary.LittleEndian.PutUint32(colBuf[138:142], col.Length)
		if col.Encoded {
			colBuf[142] = 1
		}

		if _, err := w.file.Write(colBuf); err != nil {
			return err
		}
	}

	// Write data
	for col := 0; col < len(columns); col++ {
		for _, value := range columns[col] {
			lenBuf := make([]byte, 4)
			binary.LittleEndian.PutUint32(lenBuf, uint32(len(value)))
			if _, err := w.file.Write(lenBuf); err != nil {
				return err
			}
			if len(value) > 0 {
				if _, err := w.file.Write([]byte(value)); err != nil {
					return err
				}
			}
		}
	}

	return nil
}

// Close closes the writer
func (w *Writer) Close() error {
	if w.file != nil {
		return w.file.Close()
	}
	return nil
}

// ReadFile is a convenience function to read an entire Kore file
func ReadFile(path string) ([][]string, error) {
	reader, err := NewReader(path)
	if err != nil {
		return nil, err
	}
	defer reader.Close()
	return reader.Read()
}

// WriteFile is a convenience function to write a Kore file
func WriteFile(path string, data [][]string) error {
	writer, err := NewWriter(path)
	if err != nil {
		return err
	}
	defer writer.Close()
	return writer.WriteData(data)
}

// Version returns the Kore library version
func Version() string {
	return "1.2.2"
}
