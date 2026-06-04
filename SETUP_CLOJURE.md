# Clojure Setup & Integration Guide for KORE v1.3.3

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
| JDK Version | JDK 8+ | JDK 17+ LTS | Clojure runs on JVM |
| Clojure | 1.10+ | 1.11+ | Latest stable |
| Leiningen | 2.9+ | 2.10+ | Build tool for Clojure |
| OS Support | Windows 10+ | Ubuntu 20.04+ | WSL2 for Windows |
| RAM | 2 GB | 4 GB | For compilation |
| Disk Space | 1 GB | 2 GB | JDK + Clojure |

---

## Installation

### Step 1: Install Java Development Kit

**See SETUP_JAVA.md for complete JDK installation**

```powershell
# Quick verification
java -version
javac -version
```

### Step 2: Install Leiningen

**Windows:**
```powershell
# Download lein.bat
# From: https://leiningen.org/

# Or use Chocolatey
choco install leiningen

# Verify
lein --version
```

**Linux/macOS:**
```bash
# Download and install
curl https://raw.githubusercontent.com/technomancy/leiningen/stable/bin/lein -o /usr/local/bin/lein
chmod +x /usr/local/bin/lein
lein self-install

# Verify
lein --version
```

### Step 3: Setup KORE Clojure Project

```powershell
# Create project with Leiningen
lein new app kore-clojure-tools

cd kore-clojure-tools

# Or create project.clj manually
@"
(defproject kore-clojure-tools "1.3.3"
  :description "Clojure integration tools for KORE v1.3.3"
  :url "https://github.com/arunkatherashala/Kore"
  :license {:name "MIT"}
  :dependencies [[org.clojure/clojure "1.11.1"]
                 [cheshire "5.11.0"]
                 [ring/ring-core "1.10.0"]]
  :main kore.core
  :target-path "target/%s"
  :profiles {:uberjar {:aot :all}})
"@ | Out-File -Encoding UTF8 project.clj
```

---

## Verification

### Quick Check

```powershell
# Check Clojure version
clojure --version

# Or use Leiningen
lein --version

# Create test file
@"
(println "Hello from KORE Clojure!")
(println "Clojure version:" (clojure-version))
"@ | Out-File -Encoding UTF8 test.clj

# Run
clojure test.clj

# Clean up
Remove-Item test.clj
```

### Complete Environment

```powershell
# Check Clojure REPL
clojure

# In REPL:
# user=> (clojure-version)
# "1.11.1"
# user=> (quit)
```

---

## KORE Integration

### Clojure with KORE

Clojure is excellent for:
- Functional programming with Java interop
- Data processing and transformation
- Concurrent programming with atoms/refs
- Building APIs and microservices
- Enterprise systems with functional paradigms

### Create KORE Clojure Library

**src/kore/core.clj:**
```clojure
(ns kore.core
  (:require [clojure.data.json :as json])
  (:import [java.io File]))

(def version "1.3.3")

(defrecord KoreMetadata [filename version timestamp status])

(defn process-kore-file [file-path]
  "Process a KORE file and return metadata"
  (->KoreMetadata
    (.getName (File. file-path))
    version
    (quot (System.currentTimeMillis) 1000)
    "processed"))

(defn get-metadata-json [file-path]
  "Get KORE metadata as JSON string"
  (json/write-str (process-kore-file file-path)))

(defn -main [& args]
  "Main entry point for KORE Clojure tools"
  (println (str "KORE Clojure Integration v" version))
  (println (get-metadata-json "data.kore")))
```

**src/kore/api.clj:**
```clojure
(ns kore.api
  (:require [ring.core.protocols :as protocols]
            [kore.core :as core]
            [clojure.data.json :as json]))

(defn kore-metadata-handler [request]
  "Handle GET /api/kore/metadata requests"
  {:status 200
   :headers {"Content-Type" "application/json"}
   :body (core/get-metadata-json "data.kore")})

(defn kore-version-handler [request]
  "Handle GET /api/kore/version requests"
  {:status 200
   :headers {"Content-Type" "application/json"}
   :body (json/write-str {:version core/version})})

(defn app [request]
  "KORE API application"
  (case (:uri request)
    "/api/kore/metadata" (kore-metadata-handler request)
    "/api/kore/version" (kore-version-handler request)
    {:status 404
     :headers {"Content-Type" "application/json"}
     :body (json/write-str {:error "Not found"})}))
```

---

## Common Tasks

### Building Clojure Programs

```powershell
# Compile with Leiningen
lein compile

# Build JAR
lein jar

# Build uberjar (with dependencies)
lein uberjar

# Run main function
lein run

# Run tests
lein test
```

### Running Clojure Programs

```powershell
# Start interactive REPL
clojure

# Run script
clojure -m kore.core

# Run with Leiningen
lein run

# Run specific function
lein run -m kore.core/process-kore-file
```

### Clojure REPL Examples

```clojure
; Start REPL
clojure

; Load namespace
user=> (require 'kore.core)

; Call function
user=> (kore.core/get-metadata-json "data.kore")

; Define variables
user=> (def processor (kore.core/process-kore-file "data.kore"))

; Inspect
user=> (println processor)

; Exit
user=> (quit)
```

---

## Clojure Features for KORE

### Functional Data Transformation

```clojure
(defn process-batch [file-paths]
  "Process multiple KORE files functionally"
  (map kore.core/process-kore-file file-paths))

(defn filter-large-files [file-paths min-size]
  "Filter KORE files by size"
  (filter #(> (.length (File. %)) min-size) file-paths))

; Usage
(process-batch ["file1.kore" "file2.kore" "file3.kore"])
```

### Concurrent Processing with Atoms

```clojure
(def processing-state (atom {:count 0 :errors 0}))

(defn process-concurrent [file-paths]
  "Process files concurrently, tracking state"
  (pmap
    (fn [file-path]
      (try
        (do
          (swap! processing-state update :count inc)
          (kore.core/process-kore-file file-path))
        (catch Exception e
          (swap! processing-state update :errors inc)
          {:error (.getMessage e)})))
    file-paths))

; Usage
(process-concurrent ["file1.kore" "file2.kore"])
(deref processing-state)  ; Check state
```

### Transducers for Efficient Processing

```clojure
(defn process-large-dataset [files-seq]
  "Efficiently process large datasets"
  (into
    []
    (comp
      (map kore.core/process-kore-file)
      (filter #(= (:status %) "processed")))
    files-seq))
```

---

## Troubleshooting

### Issue 1: "lein command not found"

**Solution:**
```powershell
# Download from https://leiningen.org/

# Add to PATH
$leinPath = "C:\path\to\leiningen"
[Environment]::SetEnvironmentVariable("PATH", "$env:Path;$leinPath", "User")

# Restart PowerShell
```

### Issue 2: "Cannot find JDK"

**Solution:**
```powershell
# Check JAVA_HOME
echo $env:JAVA_HOME

# Set it
[Environment]::SetEnvironmentVariable("JAVA_HOME", "C:\Program Files\Java\jdk-17", "User")

# Verify Leiningen sees it
lein version
```

### Issue 3: "Dependency not found"

**Solution:**
```powershell
# Update dependencies
lein deps

# Clear local repository
lein clean

# Rebuild
lein uberjar
```

---

## Best Practices

✅ **DO:**
- Use immutable data structures
- Leverage functional programming
- Use comprehensions for data transformation
- Test with clojure.test
- Use atoms/refs for mutable state
- Write pure functions
- Use specs for validation
- Document with docstrings

❌ **DON'T:**
- Use mutable objects unnecessarily
- Mix Java imperative style with Clojure
- Ignore type hints for performance
- Create global state
- Overuse macros
- Mix side effects with pure functions
- Ignore lazy evaluation implications
- Hardcode configuration

---

## Project Structure

```
kore-clojure-tools/
├── project.clj
├── README.md
├── src/
│   └── kore/
│       ├── core.clj
│       └── api.clj
├── test/
│   └── kore/
│       └── core_test.clj
├── target/             (generated)
│   └── kore-clojure-tools-1.3.3.jar
└── .gitignore
```

---

## Quick Reference

```powershell
# Leiningen commands
lein new app name              # Create new project
lein compile                  # Compile Clojure
lein run                      # Run main function
lein test                     # Run tests
lein jar                      # Create JAR
lein uberjar                  # Create fat JAR
lein repl                     # Start REPL
lein clean                    # Clean build
lein deps                     # Update dependencies

# REPL commands
(require 'namespace)          # Load namespace
(use 'namespace)              # Load and alias
(ns other.namespace)          # Switch namespace
(doc function-name)           # Show documentation
(source function-name)        # Show source
(quit)                        # Exit REPL
```

---

## Java Interoperability

Clojure has seamless Java interop:

```clojure
; Use Java classes from Clojure
(import 'java.io.File)

(defn read-kore-file [path]
  "Read KORE file using Java File API"
  (let [file (File. path)]
    {:exists? (.exists file)
     :readable? (.canRead file)
     :size (.length file)}))

; Call Clojure from Java
(gen-class
  :name kore.KoreProcessor
  :methods [[processFile [String] String]])

(defn -processFile [this file-path]
  (kore.core/get-metadata-json file-path))
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial Clojure setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** Architecture Documentation (Option 2)
