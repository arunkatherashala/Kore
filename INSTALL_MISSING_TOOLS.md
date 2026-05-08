# Quick Installation Guide - Missing Build Tools

## Summary
✅ 6/8 phases compiled successfully  
❌ 2 phases blocked on missing tools (Maven, SBT)  
⏳ 1 phase requires verification (Killer runtime)

---

## Option 1: Install Maven (For Phase 3 - Hadoop)

### Windows (Manual Download)
```powershell
# 1. Download Maven 3.9.x from https://maven.apache.org/download.cgi
# 2. Extract to: C:\apache-maven-3.9.x
# 3. Add to system PATH: C:\apache-maven-3.9.x\bin
# 4. Verify installation:
mvn --version

# 5. Build Phase 3:
cd hadoop
mvn clean package
```

### Windows (Using Chocolatey - if installed)
```powershell
choco install maven
mvn --version
```

### Windows (Using Windows Package Manager)
```powershell
winget install Apache.Maven
mvn --version
```

**Build Time:** ~30 seconds  
**Output:** `hadoop/target/kore-hadoop-*.jar`

---

## Option 2: Install SBT (For Phase 4 - Spark)

### Windows (Direct Download)
```powershell
# 1. Download from: https://www.scala-sbt.org/download.html
# 2. Extract to: C:\sbt
# 3. Add to PATH: C:\sbt\bin
# 4. Verify:
sbt --version

# 5. Build Phase 4:
cd spark-scala
sbt clean package
```

### Windows (Using Chocolatey)
```powershell
choco install scala sbt
sbt --version
```

### Windows (Using Scoop)
```powershell
scoop install sbt
sbt --version
```

**Build Time:** ~45 seconds (includes Scala dependency download)  
**Output:** `spark-scala/target/scala-2.12/kore-spark-*.jar`

---

## Option 3: Install Go (For Phase 6a - Go Bindings)

### Windows (Direct Download)
```powershell
# 1. Download from: https://golang.org/dl/
# 2. Run installer (e.g., go1.21.x.windows-amd64.msi)
# 3. Verify:
go version

# 4. Build Phase 6a:
cd language-bindings\go
go build ./kore
```

### Windows (Using Chocolatey)
```powershell
choco install golang
go version
```

### Windows (Using Scoop)
```powershell
scoop install go
go version
```

**Build Time:** ~10 seconds  
**Output:** `libkore.a` (static library)

---

## Option 4: Killer DSL Runtime (For Phase 6c - Killer Bindings)

### Current Status
The Killer DSL runtime availability is currently **UNKNOWN**.

### To Verify/Install
```powershell
# Check if available:
killer --version

# If not found, consult project documentation:
# - Check: README.md
# - Check: kore_fileformat_killer/README.md
# - Check: language-bindings/killer/README.md
```

### When Available
```powershell
# Execute Killer examples:
cd language-bindings\killer
killer kore_bindings.killer

# Or run specific example:
killer kore_example.killer
```

---

## All-in-One Installation Command

### Option A: Using Chocolatey
```powershell
# If you have Chocolatey installed:
choco install maven sbt golang
```

### Option B: Manual Download Links

| Tool | Link | Size |
|------|------|------|
| Maven 3.9.6 | https://archive.apache.org/dist/maven/maven-3/3.9.6/binaries/ | ~10 MB |
| SBT 1.9.8 | https://github.com/sbt/sbt/releases/download/v1.9.8/ | ~14 MB |
| Go 1.21 | https://golang.org/dl/ | ~200 MB |

---

## Quick Test Commands (After Installation)

```powershell
# Verify all tools are installed:
mvn --version
sbt --version
go version

# Build all remaining phases:
cd $workspace
cd hadoop && mvn clean package
cd ../spark-scala && sbt clean package
cd ../language-bindings/go && go build ./kore
```

---

## Expected Compile Order

```
1. Phase 3 (Hadoop) - Maven
   Command: mvn clean package
   Time: ~30s
   Output: JAR file

2. Phase 4 (Spark) - SBT
   Command: sbt clean package
   Time: ~45s
   Output: JAR file

3. Phase 6a (Go) - Go toolchain
   Command: go build ./kore
   Time: ~10s
   Output: Static library

4. Phase 6c (Killer) - Killer runtime
   Command: killer kore_bindings.killer
   Time: ~5s
   Output: Killer module
```

**Total Additional Compile Time:** ~90 seconds

---

## Verification Checklist

After installing missing tools, verify with:

```powershell
# 1. Check Maven
mvn clean compile -f hadoop/pom.xml

# 2. Check SBT
sbt -sbtVersion
cd spark-scala && sbt compile

# 3. Check Go
go build -v ./language-bindings/go/kore

# 4. Run full test suite
.\test_suite.ps1
```

---

## Current Status After Installation

After installing Maven, SBT, and Go:
- ✅ Phase 2 (PyO3): Already compiled
- ✅ Phase 3 (Hadoop): Will compile with Maven
- ✅ Phase 4 (Spark): Will compile with SBT
- ✅ Phase 5 (Python): Already validated
- ✅ Phase 6a (Go): Will compile with Go toolchain
- ✅ Phase 6b (Java): Already compiled to bytecode
- ❓ Phase 6c (Killer): Depends on runtime availability
- ✅ Phase 7 (Query Opt): Already compiled

**Target:** 100% Compilation Success (7/8 minimum, 8/8 if Killer available)

---

## Support

For detailed compilation information, see: `COMPILATION_REPORT.md`

