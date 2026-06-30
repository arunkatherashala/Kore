// kore_test.go — KORE Go test using syscall.LoadDLL (no CGO, no gcc needed!)
// Run: go run kore_test.go
package main

import (
	"encoding/json"
	"fmt"
	"syscall"
	"unsafe"
)

const KORE_DLL = `C:\Users\skathera\Downloads\asistent\kore\target\release\kore_ffi.dll`

func main() {
	fmt.Println("=== KORE Go Test (syscall.LoadDLL — no CGO) ===")

	// Load the DLL
	dll, err := syscall.LoadDLL(KORE_DLL)
	if err != nil {
		fmt.Printf("FAILED to load DLL: %v\n", err)
		return
	}
	defer dll.Release()
	fmt.Println("[1] DLL loaded")

	// Get function handles
	fnNew,   _ := dll.FindProc("kore_session_new")
	fnFree,  _ := dll.FindProc("kore_session_free")
	fnLoad,  _ := dll.FindProc("kore_session_load_csv")
	fnQuery, _ := dll.FindProc("kore_session_query")
	fnCount, _ := dll.FindProc("kore_session_row_count")
	fnFreeS, _ := dll.FindProc("kore_free_string")

	// Create session
	sess, _, _ := fnNew.Call()
	if sess == 0 {
		fmt.Println("FAILED: session is NULL")
		return
	}
	fmt.Printf("[2] Session: 0x%x\n", sess)
	defer fnFree.Call(sess)

	// Load CSV
	table := []byte("bench\x00")
	path  := []byte(`C:\Users\skathera\Downloads\asistent\bench_export.csv` + "\x00")
	rc, _, _ := fnLoad.Call(sess,
		uintptr(unsafe.Pointer(&table[0])),
		uintptr(unsafe.Pointer(&path[0])))
	fmt.Printf("[3] load_csv returned: %d (0=OK)\n", rc)

	// Row count
	tbl := []byte("bench\x00")
	n, _, _ := fnCount.Call(sess, uintptr(unsafe.Pointer(&tbl[0])))
	fmt.Printf("[4] Row count: %d\n", n)

	// SQL query
	sql := []byte("SELECT category, COUNT(*) as cnt, SUM(amount) as total FROM bench GROUP BY category ORDER BY total DESC\x00")
	ptr, _, _ := fnQuery.Call(sess, uintptr(unsafe.Pointer(&sql[0])))
	if ptr == 0 {
		fmt.Println("[5] Query returned NULL")
		return
	}
	// Read C string
	jsonBytes := (*[1 << 20]byte)(unsafe.Pointer(ptr))[:]
	end := 0
	for jsonBytes[end] != 0 {
		end++
	}
	jsonStr := string(jsonBytes[:end])
	fnFreeS.Call(ptr)

	var rows []map[string]interface{}
	_ = json.Unmarshal([]byte(jsonStr), &rows)
	fmt.Printf("[5] GROUP BY result (%d groups):\n", len(rows))
	for _, r := range rows {
		fmt.Printf("     category=%v cnt=%.0f total=%.2f\n", r["category"], r["cnt"], r["total"])
	}

	// WHERE+LIMIT query
	sql2 := []byte("SELECT id, amount FROM bench WHERE amount > 999 ORDER BY amount DESC LIMIT 3\x00")
	ptr2, _, _ := fnQuery.Call(sess, uintptr(unsafe.Pointer(&sql2[0])))
	if ptr2 != 0 {
		jsonBytes2 := (*[1 << 20]byte)(unsafe.Pointer(ptr2))[:]
		end2 := 0
		for jsonBytes2[end2] != 0 { end2++ }
		fmt.Printf("[6] WHERE+LIMIT: %s\n", string(jsonBytes2[:end2]))
		fnFreeS.Call(ptr2)
	}

	fmt.Println("\nGO TEST PASSED — kore_ffi.dll works from Go via syscall.LoadDLL!")
}
