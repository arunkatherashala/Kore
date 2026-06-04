# Go Setup & Integration Guide for KORE v1.3.3

**Last Updated:** June 3, 2026  
**Status:** Production Ready  
**Version:** v1.0

---

## 📋 Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Verification](#verification)
4. [KORE Integration](#kore-integration)
5. [Common Tasks](#common-tasks)
6. [Troubleshooting](#troubleshooting)

---

## Prerequisites

| Requirement | Minimum | Recommended | Notes |
|-------------|---------|-------------|-------|
| Go Version | 1.18+ | 1.21+ | Latest stable version |
| OS Support | Windows 10+ | Windows 10, 11 | Also supports Linux/macOS |
| RAM | 1 GB | 2 GB | For compilation |
| Disk Space | 500 MB | 1 GB | Go SDK + binaries |

---

## Installation

### Step 1: Download Go

**Official Website:** https://golang.org/dl

**Windows Installation:**
```powershell
# Option 1: Manual download
# Go to https://golang.org/dl
# Download Windows installer (MSI)

# Option 2: Windows Package Manager
winget install GoLang.Go

# Option 3: Chocolatey
choco install golang
```

### Step 2: Install Go

```powershell
# Run installer and follow on-screen instructions
# Default installation directory: C:\Program Files\Go

# Or install via package manager
winget install GoLang.Go
```

### Step 3: Verify Installation

```powershell
# Check Go version
go version

# Check go environment
go env

# Expected output:
# go version go1.21.0 windows/amd64
```

---

## Verification

### Quick Check
```powershell
# Test Go installation
go version

# Create test program
@"
package main
import "fmt"
func main() {
    fmt.Println("Go is working!")
}
"@ | Out-File -Encoding UTF8 test.go

# Build and run
go run test.go

# Clean up
Remove-Item test.go
```

### Complete Environment Check

```powershell
# Show Go environment variables
go env

# List available commands
go help

# Check Go modules
go mod help

# Test compilation
go version
```

---

## KORE Integration

### Go with KORE

Go can integrate with KORE for:
- High-performance services
- CLI tools
- Distributed systems
- Microservices
- REST APIs
- Data pipeline tools

### Setup KORE Go Project

**Step 1: Create Project**

```powershell
# Create project directory
mkdir kore-go-tools
cd kore-go-tools

# Initialize Go module
go mod init github.com/kore/kore-go-tools
```

**Step 2: Create Main Program**

Create `main.go`:
```go
package main

import (
    "fmt"
    "log"
)

func main() {
    fmt.Println("KORE Go Integration v1.3.3")
    fmt.Println("Starting KORE service...")
    
    processor, err := NewKoreProcessor("data.kore")
    if err != nil {
        log.Fatalf("Failed to initialize processor: %v", err)
    }
    
    metadata, err := processor.ProcessFile()
    if err != nil {
        log.Fatalf("Failed to process file: %v", err)
    }
    
    fmt.Printf("Processed file: %s\n", metadata.Filename)
    fmt.Printf("Version: %s\n", metadata.Version)
}

type KoreMetadata struct {
    Filename string
    Version  string
}

type KoreProcessor struct {
    filePath string
}

func NewKoreProcessor(filePath string) (*KoreProcessor, error) {
    return &KoreProcessor{filePath: filePath}, nil
}

func (kp *KoreProcessor) ProcessFile() (*KoreMetadata, error) {
    return &KoreMetadata{
        Filename: kp.filePath,
        Version:  "1.3.3",
    }, nil
}
```

**Step 3: Build Project**

```powershell
# Build executable
go build

# Run executable
.\kore-go-tools.exe

# Or run directly
go run main.go
```

---

## Common Tasks

### Building Go Programs

```powershell
# Build for current platform
go build

# Build with custom output name
go build -o kore.exe

# Build for different platforms
go build -o kore-linux -goos linux -goarch amd64
go build -o kore-mac -goos darwin -goarch amd64

# Build with version info
go build -ldflags "-X main.Version=1.3.3"

# Optimize binary size
go build -ldflags "-s -w"
```

### Managing Dependencies

```powershell
# Initialize module
go mod init package/name

# Add dependency (automatic when imported)
go get github.com/some/package

# Update all dependencies
go get -u ./...

# Update specific dependency
go get -u github.com/some/package@latest

# Remove unused dependencies
go mod tidy

# Verify dependencies
go mod verify

# List all dependencies
go list -m all
```

### Testing in Go

```powershell
# Run all tests
go test ./...

# Run with verbose output
go test -v ./...

# Run specific test
go test -run TestName

# Run with coverage
go test -cover ./...

# Generate coverage report
go test -coverprofile=coverage.out ./...
go tool cover -html=coverage.out
```

### Code Quality

```powershell
# Format code
go fmt ./...

# Lint code (requires golangci-lint)
golangci-lint run ./...

# Or use gofmt
gofmt -s -w .

# Vet code for errors
go vet ./...
```

---

## KORE Service Example

**main.go** - Full REST API for KORE:
```go
package main

import (
    "encoding/json"
    "fmt"
    "log"
    "net/http"
    "os"
)

type KoreFile struct {
    Filename string `json:"filename"`
    Version  string `json:"version"`
    Size     int64  `json:"size"`
}

func main() {
    http.HandleFunc("/api/kore/metadata", handleMetadata)
    http.HandleFunc("/api/kore/health", handleHealth)
    
    port := ":8080"
    log.Printf("KORE server starting on %s", port)
    log.Fatal(http.ListenAndServe(port, nil))
}

func handleMetadata(w http.ResponseWriter, r *http.Request) {
    filename := r.URL.Query().Get("file")
    if filename == "" {
        http.Error(w, "file parameter required", http.StatusBadRequest)
        return
    }
    
    // Get file stats
    stat, err := os.Stat(filename)
    if err != nil {
        http.Error(w, "file not found", http.StatusNotFound)
        return
    }
    
    kf := KoreFile{
        Filename: filename,
        Version:  "1.3.3",
        Size:     stat.Size(),
    }
    
    w.Header().Set("Content-Type", "application/json")
    json.NewEncoder(w).Encode(kf)
}

func handleHealth(w http.ResponseWriter, r *http.Request) {
    health := map[string]interface{}{
        "status":  "healthy",
        "version": "1.3.3",
    }
    w.Header().Set("Content-Type", "application/json")
    json.NewEncoder(w).Encode(health)
}
```

---

## Troubleshooting

### Issue 1: "go is not recognized"

**Solution:**
```powershell
# Check PATH
$env:Path -split ';' | Select-String 'Go'

# Add Go to PATH (if using older version)
$env:Path += ";C:\Program Files\Go\bin"

# Restart PowerShell
```

### Issue 2: "Module github.com not allowed"

**Solution:**
```powershell
# Enable go modules
go env -w GO111MODULE=on

# Or in go.mod, change module path to local
# module kore-go-tools
```

### Issue 3: "Build fails with linker error"

**Solution:**
```powershell
# Clean build cache
go clean -cache

# Rebuild
go build

# Or check dependencies
go mod tidy
go get -u ./...
```

### Issue 4: "Memory issues during build"

**Solution:**
```powershell
# Go normally handles memory well
# But if issues persist:
go clean -cache
go build -p 1  # Serialize build (slower but uses less memory)
```

---

## Best Practices

✅ **DO:**
- Follow Go naming conventions (camelCase for functions)
- Use `go fmt` for consistent formatting
- Write tests alongside code
- Use `go vet` before committing
- Handle errors explicitly
- Use interfaces for abstraction
- Keep GOPATH clean
- Use Go modules (go.mod)

❌ **DON'T:**
- Ignore error returns
- Use `panic()` in libraries
- Create global state
- Write long functions
- Use reflection excessively
- Ignore goroutine leaks
- Use `goto` statements
- Skip tests

---

## Project Structure

```
kore-go-tools/
├── go.mod
├── go.sum
├── main.go
├── cmd/
│   └── kore-cli/
│       └── main.go
├── internal/
│   ├── processor/
│   │   ├── processor.go
│   │   └── processor_test.go
│   └── api/
│       ├── server.go
│       └── server_test.go
├── pkg/
│   └── kore/
│       └── types.go
└── README.md
```

---

## Quick Reference

```powershell
# Basics
go version                      # Check Go version
go env                          # Show environment
go help                         # Show help

# Module management
go mod init package-name        # Initialize module
go mod tidy                     # Remove unused dependencies
go mod verify                   # Verify dependencies
go mod download                 # Download modules
go list -m all                  # List dependencies

# Building
go build                        # Build executable
go build -o name.exe           # Build with custom name
go run main.go                 # Run directly
go install                     # Install binary

# Testing
go test ./...                  # Run all tests
go test -v ./...               # Verbose test output
go test -cover ./...           # Show coverage
go test -race ./...            # Detect race conditions

# Code quality
go fmt ./...                   # Format code
go vet ./...                   # Check for errors
golangci-lint run              # Run linter

# Dependencies
go get package-name            # Get package
go get -u ./...                # Update all packages
go clean -modcache             # Clear mod cache
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** Java Setup & Integration Guide (coming next)
