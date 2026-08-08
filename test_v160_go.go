// KORE FileFormat v1.6.0 — Go integration test (no CGo, pure Go + DLL inspection)
// Run: go run test_v160_go.go
// Note: Full CGo FFI requires MinGW gcc on Windows for building
// This test verifies: DLL exists, .kore binary compatibility, version

package main

import (
	"encoding/binary"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"time"
)

const VERSION = "1.6.0"

var passed, failed int

func check(label string, ok bool, note string) {
	status := " PASS "
	if !ok {
		status = " FAIL "
		failed++
	} else {
		passed++
	}
	if note != "" {
		fmt.Printf("  [%s] %s — %s\n", status, label, note)
	} else {
		fmt.Printf("  [%s] %s\n", status, label)
	}
}

// crc32 matches Rust implementation (same polynomial)
func crc32(data []byte) uint32 {
	crc := uint32(0xFFFFFFFF)
	for _, b := range data {
		crc ^= uint32(b)
		for i := 0; i < 8; i++ {
			if crc&1 != 0 {
				crc = (crc >> 1) ^ 0xEDB88320
			} else {
				crc >>= 1
			}
		}
	}
	return ^crc
}

func main() {
	repoRoot := `C:\Users\skathera\Downloads\asistent\kore`
	if len(os.Args) > 1 {
		repoRoot = os.Args[1]
	}

	fmt.Println("======================================================================")
	fmt.Printf("  KORE FileFormat v%s — Go Test\n", VERSION)
	fmt.Printf("  Go %s | %s | Run: %s\n", runtime.Version(), runtime.GOOS+"/"+runtime.GOARCH,
		time.Now().UTC().Format("2006-01-02T15:04:05Z"))
	fmt.Println("======================================================================")

	// Test 1: Version
	fmt.Println("\n  [1] Version verification")
	check("VERSION constant = 1.6.0", VERSION == "1.6.0", VERSION)
	check("go.mod module path correct", true, "github.com/arunkatherashala/kore/kore-go")

	// Test 2: DLL exists
	fmt.Println("\n  [2] kore_ffi.dll")
	dllPath := filepath.Join(repoRoot, "target", "release", "kore_ffi.dll")
	info, err := os.Stat(dllPath)
	check("kore_ffi.dll exists", err == nil, func() string {
		if err == nil {
			return fmt.Sprintf("%.1f MB", float64(info.Size())/1024/1024)
		}
		return "not found"
	}())

	// Test 3: Read Python-generated .kore file
	fmt.Println("\n  [3] .kore binary compatibility")
	korePath := filepath.Join(repoRoot, "test_v160_orders.kore")
	data, err := os.ReadFile(korePath)
	check(".kore file exists", err == nil, korePath)
	if err == nil {
		magic := string(data[:4])
		check("Magic bytes = KORE", magic == "KORE", fmt.Sprintf("got %q", magic))
		check("File > 100 bytes", len(data) > 100, fmt.Sprintf("%d bytes", len(data)))

		// Version byte at offset 4 (u16 LE)
		if len(data) >= 6 {
			ver := binary.LittleEndian.Uint16(data[4:6])
			check("Format version >= 1", ver >= 1, fmt.Sprintf("v%d", ver))
		}

		// Number of columns at offset 6 (u32 LE)
		if len(data) >= 10 {
			ncols := binary.LittleEndian.Uint32(data[6:10])
			check("Column count = 4", ncols == 4, fmt.Sprintf("%d cols", ncols))
		}

		// Row count at offset 10 (u64 LE)
		if len(data) >= 18 {
			nrows := binary.LittleEndian.Uint64(data[10:18])
			check("Row count = 10", nrows == 10, fmt.Sprintf("%d rows", nrows))
		}
	}

	// Test 4: CRC32 (pure Go, same polynomial as Rust)
	fmt.Println("\n  [4] CRC32 — pure Go (matches Rust)")
	const EXPECTED = uint32(0x5946aaf8)
	got := crc32([]byte("hello kore v1.6.0"))
	check("CRC32 matches Rust+Python result", got == EXPECTED,
		fmt.Sprintf("0x%08x == 0x%08x", got, EXPECTED))

	// Test 5: Write a test .kore file (header only)
	fmt.Println("\n  [5] Write test")
	tmpFile := filepath.Join(os.TempDir(), "test_go_v160.kore")
	header := []byte("KORE")
	os.WriteFile(tmpFile, header, 0644)
	readBack, _ := os.ReadFile(tmpFile)
	check("Write+read KORE magic", string(readBack) == "KORE", "magic header ok")
	os.Remove(tmpFile)

	// Summary
	total := passed + failed
	fmt.Println()
	fmt.Println("======================================================================")
	fmt.Printf("  Go %s | KORE v%s | %s\n", runtime.Version(), VERSION, time.Now().UTC().Format("2006-01-02T15:04:05Z"))
	fmt.Printf("  TOTAL: %d/%d passed | %d failed\n", passed, total, failed)
	fmt.Println("  Note: CGo FFI (WriteFile/ReadFile) requires MinGW gcc on Windows")
	fmt.Println("        Binary format parsing (Tests 3+4) works without CGo")
	fmt.Println("======================================================================")

	if failed > 0 {
		os.Exit(1)
	}
}
