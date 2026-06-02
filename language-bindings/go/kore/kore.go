/*
Package kore provides Go bindings for the Kore columnar file format.

Enables reading/writing Kore files directly from Go applications.

Example:

reader, err := kore.NewReader("data.kore")
if err != nil {
log.Fatal(err)
}
defer reader.Close()

data, err := reader.Read()
if err != nil {
log.Fatal(err)
}

// data is [][]string (columns x rows)
for colIdx, column := range data {
fmt.Printf("Column %d: %v rows\n", colIdx, len(column))
}
*/
package kore
