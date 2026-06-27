package main

import (
	"fmt"
	"log"

	"github.com/arunkatherashala/kore-go/kore"
)

func main() {
	// Example 1: Reading a Kore file
	fmt.Println("=== Reading Kore File ===")
	reader, err := kore.NewReader("data.kore")
	if err != nil {
		log.Fatal(err)
	}
	defer reader.Close()

	// Read all data
	data, err := reader.Read()
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Columns: %d, Rows: %d\n", len(data), len(data[0]))
	for colIdx, column := range data {
		fmt.Printf("Column %d: %d rows\n", colIdx, len(column))
		if len(column) > 0 {
			fmt.Printf("  First value: %v\n", column[0])
		}
	}

	// Example 2: Reading a specific column
	fmt.Println("\n=== Reading Specific Column ===")
	reader2, err := kore.NewReader("data.kore")
	if err != nil {
		log.Fatal(err)
	}
	defer reader2.Close()

	column, err := reader2.ReadColumn(0)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("First column has %d rows\n", len(column))

	// Example 3: Writing a Kore file
	fmt.Println("\n=== Writing Kore File ===")
	data = [][]string{
		{"Alice", "Bob", "Charlie"},
		{"25", "30", "35"},
		{"Engineer", "Designer", "Manager"},
	}

	err = kore.WriteFile("output.kore", data)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("Successfully wrote output.kore")

	// Example 4: Reading the file we just wrote
	fmt.Println("\n=== Reading Written File ===")
	data, err = kore.ReadFile("output.kore")
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Read %d columns with %d rows each\n", len(data), len(data[0]))
	for colIdx, column := range data {
		fmt.Printf("Column %d: %v\n", colIdx, column)
	}

	fmt.Printf("\nKore library version: %s\n", kore.Version())
}
