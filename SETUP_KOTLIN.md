# Kotlin Setup & Integration Guide for KORE v1.3.3

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
| JDK Version | JDK 8+ | JDK 17+ LTS | Kotlin runs on JVM |
| Kotlin | 1.8+ | 1.9+ | Latest stable |
| Build Tool | Maven/Gradle | Gradle 7.0+ | Kotlin native support |
| OS Support | Windows 10+ | Windows 10, 11 | Also supports Linux/macOS |
| RAM | 2 GB | 4 GB | For compilation |
| Disk Space | 1 GB | 2 GB | JDK + Kotlin |

---

## Installation

### Step 1: Install Java Development Kit

**See SETUP_JAVA.md for complete JDK installation**

Quick verification:
```powershell
# Check Java installation
java -version
javac -version
```

### Step 2: Install Kotlin

**Official Download:**
```powershell
# From: https://kotlinlang.org/docs/command-line.html

# Or use Chocolatey (Windows)
choco install kotlin

# Or use SDKMAN (cross-platform)
# See: https://sdkman.io/
```

**Manual Installation (Windows):**
```powershell
# Download from GitHub releases
# https://github.com/JetBrains/kotlin/releases

# Extract to installation directory
Expand-Archive -Path "kotlin-compiler-1.9.x.zip" -DestinationPath "C:\kotlin"

# Add to PATH
$env:Path += ";C:\kotlin\bin"
```

### Step 3: Verify Installation

```powershell
# Check Kotlin version
kotlinc -version

# Expected:
# Kotlin compiler version 1.9.x
```

---

## Verification

### Quick Check
```powershell
# Check Kotlin compiler
kotlinc -version

# Create test Kotlin file
@"
fun main() {
    println("Kotlin is working!")
}
"@ | Out-File -Encoding UTF8 Test.kt

# Compile
kotlinc Test.kt -include-runtime -d Test.jar

# Run
java -jar Test.jar

# Clean up
Remove-Item Test.kt, Test.jar
```

### Complete Environment Check

```powershell
# Show Kotlin compiler info
kotlinc -version

# Test REPL (interactive mode)
# Type: println("Hello")
# Then: exit
# kotlinc -J-XX:+IgnoreUnrecognizedVMOptions
```

---

## KORE Integration

### Kotlin with KORE

Kotlin offers advantages for KORE integration:
- More concise than Java (but full Java interop)
- Null safety built-in
- Extension functions for cleaner APIs
- Coroutines for async operations
- Spring Boot Kotlin support
- Data classes for KORE metadata

### Setup KORE Kotlin Project with Gradle

**Step 1: Create Kotlin Project**

```powershell
# Navigate to KORE directory
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"

# Create directory
mkdir kore-kotlin-tools
cd kore-kotlin-tools

# Create build.gradle.kts
@"
plugins {
    kotlin("jvm") version "1.9.0"
    application
}

group = "com.kore"
version = "1.3.3"

repositories {
    mavenCentral()
}

dependencies {
    // JSON processing
    implementation("com.google.code.gson:gson:2.10.1")
    
    // Testing
    testImplementation("org.junit.jupiter:junit-jupiter:5.9.2")
    testImplementation("org.junit.jupiter:junit-jupiter-api:5.9.2")
    testRuntimeOnly("org.junit.jupiter:junit-jupiter-engine:5.9.2")
}

application {
    mainClass.set("com.kore.AppKt")
}

tasks.test {
    useJUnitPlatform()
}
"@ | Out-File -Encoding UTF8 build.gradle.kts

# Create src structure
mkdir -p src/main/kotlin/com/kore
mkdir -p src/test/kotlin/com/kore
```

**Step 2: Create Kotlin Files**

**src/main/kotlin/com/kore/App.kt:**
```kotlin
package com.kore

data class KoreMetadata(
    val filename: String,
    val version: String,
    val timestamp: Long = System.currentTimeMillis()
)

class KoreProcessor(private val filePath: String) {
    fun processFile(): KoreMetadata {
        return KoreMetadata(filePath, "1.3.3")
    }
    
    fun getMetadataJson(): String {
        return buildString {
            append("{\n")
            append("  \"filename\": \"${processFile().filename}\",\n")
            append("  \"version\": \"${processFile().version}\"\n")
            append("}")
        }
    }
}

fun main() {
    println("KORE Kotlin Integration v1.3.3")
    
    val processor = KoreProcessor("data.kore")
    println(processor.getMetadataJson())
}
```

**Step 3: Build and Run**

```powershell
# Build project
gradle build

# Run application
gradle run

# Create executable JAR
gradle jar

# Run JAR
java -jar build/libs/kore-kotlin-tools-1.3.3.jar
```

---

## Common Tasks

### Building Kotlin Programs

```powershell
# Compile with Gradle
gradle build

# Build without tests
gradle build -x test

# Create JAR
gradle jar

# Create fat JAR (includes dependencies)
gradle shadowJar  # (requires shadow plugin)

# Build with Kotlin compiler directly
kotlinc src/main/kotlin -include-runtime -d app.jar
```

### Running Kotlin Programs

```powershell
# Run with Gradle
gradle run

# Run JAR
java -jar build/libs/app.jar

# Run Kotlin file directly
kotlinc -script script.kts

# Interactive REPL
kotlinc
# Then type Kotlin code
```

### Testing Kotlin Code

```powershell
# Run tests
gradle test

# Run specific test
gradle test --tests "com.kore.AppTest"

# Run with coverage (requires jacoco plugin)
gradle jacocoTestReport
```

### Kotlin REPL for Interactive Development

```powershell
# Start REPL
kotlinc

# Example interactive session:
# >>> data class Person(val name: String, val age: Int)
# >>> val p = Person("Alice", 30)
# >>> println(p)
# Person(name=Alice, age=30)
# >>> :quit
```

---

## Kotlin Features for KORE

### Data Classes

```kotlin
data class KoreFile(
    val id: Int,
    val filename: String,
    val sizeBytes: Long,
    val version: String,
    val createdAt: Long = System.currentTimeMillis()
)

// Automatically generates:
// - equals(), hashCode(), toString()
// - copy() function
// - destructuring
val file = KoreFile(1, "data.kore", 1024000, "1.3.3")
println(file)  // KoreFile(id=1, filename=data.kore, ...)
```

### Null Safety

```kotlin
fun processKoreFile(filename: String?): String {
    // filename can be null
    return filename?.let { 
        "Processing: $it" 
    } ?: "No filename provided"
}
```

### Extension Functions

```kotlin
fun String.isKoreFile(): Boolean = this.endsWith(".kore")

// Usage
if ("data.kore".isKoreFile()) {
    println("This is a KORE file!")
}
```

### Coroutines for Async Operations

```kotlin
import kotlinx.coroutines.*

suspend fun processKoreFileAsync(filename: String): KoreMetadata {
    return withContext(Dispatchers.IO) {
        // Async file processing
        KoreMetadata(filename, "1.3.3")
    }
}

fun main() = runBlocking {
    val result = processKoreFileAsync("data.kore")
    println(result)
}
```

---

## Troubleshooting

### Issue 1: "kotlinc is not recognized"

**Solution:**
```powershell
# Check installation path
where kotlinc

# Add Kotlin to PATH
$env:Path += ";C:\kotlin\bin"

# Or use Gradle instead
gradle build
```

### Issue 2: "Failed to find class"

**Solution:**
```powershell
# Clean and rebuild
gradle clean build

# Or check main class
# In build.gradle.kts:
# application {
#     mainClass.set("com.kore.AppKt")
# }
```

### Issue 3: "Gradle wrapper not found"

**Solution:**
```powershell
# Install Gradle
choco install gradle

# Or create Gradle wrapper
gradle wrapper --gradle-version 8.0

# Use wrapper
.\gradlew build  # Windows
./gradlew build  # Mac/Linux
```

### Issue 4: "Kotlin plugin not found"

**Solution:**
```powershell
# Ensure Gradle has internet (for downloads)
gradle build

# Or specify explicit version
# In build.gradle.kts:
# plugins {
#     kotlin("jvm") version "1.9.0"
# }
```

---

## Best Practices

✅ **DO:**
- Use data classes for POJOs
- Leverage null safety (?: operator)
- Use extension functions for cleaner APIs
- Write concise lambda expressions
- Use `when` instead of `if-else` chains
- Utilize Kotlin's type inference
- Document public APIs with KDoc
- Use scope functions (let, apply, run)

❌ **DON'T:**
- Use `!!` operator excessively (null assertion)
- Mix Kotlin idioms with Java patterns
- Create overly nested lambdas
- Ignore compiler warnings
- Use `var` when `val` would work
- Create mutable global state
- Overuse reflection
- Ignore IntelliJ IDEA inspection hints

---

## Project Structure

```
kore-kotlin-tools/
├── build.gradle.kts
├── gradle/
│   └── wrapper/
├── src/
│   ├── main/
│   │   ├── kotlin/
│   │   │   └── com/kore/
│   │   │       └── App.kt
│   │   └── resources/
│   └── test/
│       └── kotlin/
│           └── com/kore/
│               └── AppTest.kt
├── build/                  (generated)
│   ├── classes/
│   └── libs/
│       └── kore-kotlin-tools-1.3.3.jar
└── README.md
```

---

## Quick Reference

```powershell
# Compilation
kotlinc HelloWorld.kt                  # Compile Kotlin file
kotlinc src/ -d bin                    # Compile directory
kotlinc src/ -include-runtime -d app.jar  # Create JAR

# Execution
kotlin HelloKt                         # Run Kotlin class
java -jar app.jar                      # Run JAR
kotlinc -script script.kts             # Run script

# Gradle commands
gradle build                           # Build project
gradle run                             # Run application
gradle test                            # Run tests
gradle jar                             # Create JAR
gradle clean                           # Clean build

# Interactive
kotlinc                                # Start REPL
# (Type :quit to exit)
```

---

## Java Interoperability

Kotlin has perfect Java interoperability:

```kotlin
// Use Java classes from Kotlin
import java.io.File

fun readFile(path: String): String {
    return File(path).readText()
}

// Call Kotlin from Java
public class JavaApp {
    public static void main(String[] args) {
        String content = AppKt.readFile("data.kore");
        System.out.println(content);
    }
}
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Note:** Kotlin is 100% interoperable with Java, making it ideal for KORE integration alongside Java components.
