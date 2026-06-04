# Java Setup & Integration Guide for KORE v1.3.3

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
| JDK Version | JDK 8+ | JDK 17+ LTS | Oracle or OpenJDK |
| Java Compiler | javac 8+ | javac 17+ | Included with JDK |
| Build Tool | Maven/Gradle | Maven 3.9+ | Dependency management |
| OS Support | Windows 10+ | Windows 10, 11 | Also supports Linux/macOS |
| RAM | 2 GB | 4 GB | For compilation |
| Disk Space | 1 GB | 2 GB | JDK + tools |

---

## Installation

### Step 1: Install Java Development Kit (JDK)

**Official Download:**
```powershell
# From Oracle: https://www.oracle.com/java/technologies/downloads/
# Or OpenJDK: https://adoptium.net/

# Windows Package Manager
winget install Oracle.JDK.17

# Or Chocolatey
choco install jdk17
```

**Verify Installation:**
```powershell
# Check Java version
java -version

# Check compiler version
javac -version

# Expected:
# java version "17.0.x" (or newer)
# javac 17.0.x
```

### Step 2: Set JAVA_HOME Environment Variable

```powershell
# Find Java installation path
where java

# Set JAVA_HOME (adjust path based on installation)
[Environment]::SetEnvironmentVariable("JAVA_HOME", "C:\Program Files\Java\jdk-17", "User")

# Add to PATH
$currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
$newPath = $currentPath + ";$env:JAVA_HOME\bin"
[Environment]::SetEnvironmentVariable("PATH", $newPath, "User")

# Verify
echo $env:JAVA_HOME
```

### Step 3: Install Build Tools

**Maven (recommended for KORE integration):**
```powershell
# See SETUP_MAVEN.md for complete Maven installation
# Or quick install:
winget install Apache.Maven

# Verify
mvn --version
```

---

## Verification

### Quick Check
```powershell
# Check Java
java -version

# Check compiler
javac -version

# Create test class
@"
public class Test {
    public static void main(String[] args) {
        System.out.println("Java is working!");
    }
}
"@ | Out-File Test.java

# Compile
javac Test.java

# Run
java Test

# Clean up
Remove-Item Test.java, Test.class
```

### Complete Environment Check

```powershell
# Show Java details
java --version

# Show compiler details
javac --version

# List all Java details
java -XshowSettings:all -version
```

---

## KORE Integration

### Java with KORE

Java can integrate with KORE for:
- High-performance data processing
- Enterprise application integration
- Spring Boot microservices
- Data analytics applications
- Testing frameworks
- Build pipeline integration

### Setup KORE Java Project

**Step 1: Create Maven Project**

```powershell
# Navigate to KORE directory
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"

# Create Java project
mvn archetype:generate -DgroupId=com.kore -DartifactId=kore-java-tools -DarchetypeArtifactId=maven-archetype-quickstart -DinteractiveMode=false

cd kore-java-tools
```

**Step 2: Configure pom.xml**

```xml
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 
                             http://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  
  <groupId>com.kore</groupId>
  <artifactId>kore-java-tools</artifactId>
  <version>1.3.3</version>
  <packaging>jar</packaging>
  
  <name>KORE Java Tools</name>
  <description>Java integration tools for KORE v1.3.3</description>
  
  <properties>
    <maven.compiler.source>17</maven.compiler.source>
    <maven.compiler.target>17</maven.compiler.target>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  
  <dependencies>
    <!-- JSON processing -->
    <dependency>
      <groupId>com.google.code.gson</groupId>
      <artifactId>gson</artifactId>
      <version>2.10.1</version>
    </dependency>
    
    <!-- Testing -->
    <dependency>
      <groupId>junit</groupId>
      <artifactId>junit</artifactId>
      <version>4.13.2</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
  
  <build>
    <plugins>
      <plugin>
        <groupId>org.apache.maven.plugins</groupId>
        <artifactId>maven-compiler-plugin</artifactId>
        <version>3.11.0</version>
        <configuration>
          <source>17</source>
          <target>17</target>
        </configuration>
      </plugin>
      
      <plugin>
        <groupId>org.apache.maven.plugins</groupId>
        <artifactId>maven-jar-plugin</artifactId>
        <version>3.3.0</version>
        <configuration>
          <archive>
            <manifest>
              <mainClass>com.kore.App</mainClass>
            </manifest>
          </archive>
        </configuration>
      </plugin>
    </plugins>
  </build>
</project>
```

---

## Common Tasks

### Building Java Projects

```powershell
# Compile with javac
javac -d build src/com/kore/*.java

# Compile with Maven
mvn clean compile

# Build JAR
mvn clean package

# Build without tests
mvn clean package -DskipTests

# Build and install locally
mvn clean install
```

### Running Java Programs

```powershell
# Direct execution
java -cp .:lib/* com.kore.App

# Using Maven
mvn exec:java -Dexec.mainClass="com.kore.App"

# Running JAR
java -jar target/kore-java-tools-1.3.3.jar
```

### Testing Java Code

```powershell
# Run tests with Maven
mvn test

# Run specific test
mvn test -Dtest=AppTest

# Generate coverage report
mvn test jacoco:report

# View report
Start-Process target/site/jacoco/index.html
```

### KORE Java Integration Example

**src/main/java/com/kore/KoreProcessor.java:**
```java
package com.kore;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;

public class KoreProcessor {
    private String filePath;
    private static final Gson gson = new GsonBuilder().setPrettyPrinting().create();
    
    public KoreProcessor(String filePath) {
        this.filePath = filePath;
    }
    
    public KoreMetadata processFile() {
        return new KoreMetadata(filePath, "1.3.3");
    }
    
    public String getMetadataJson() {
        return gson.toJson(processFile());
    }
    
    public static class KoreMetadata {
        public String filename;
        public String version;
        public long timestamp;
        
        public KoreMetadata(String filename, String version) {
            this.filename = filename;
            this.version = version;
            this.timestamp = System.currentTimeMillis();
        }
    }
}
```

**src/main/java/com/kore/App.java:**
```java
package com.kore;

public class App {
    public static void main(String[] args) {
        System.out.println("KORE Java Integration v1.3.3");
        
        KoreProcessor processor = new KoreProcessor("data.kore");
        System.out.println(processor.getMetadataJson());
    }
}
```

---

## Troubleshooting

### Issue 1: "java is not recognized"

**Solution:**
```powershell
# Check JAVA_HOME
echo $env:JAVA_HOME

# Set it if not set
[Environment]::SetEnvironmentVariable("JAVA_HOME", "C:\Program Files\Java\jdk-17", "User")

# Add to PATH
$env:Path += ";$env:JAVA_HOME\bin"

# Restart PowerShell
```

### Issue 2: "javac version mismatch"

**Solution:**
```powershell
# Check Java version
java -version

# Check compiler version
javac -version

# They should match. If not, reinstall JDK or set JAVA_HOME correctly
```

### Issue 3: "Maven can't find Java"

**Solution:**
```powershell
# Set JAVA_HOME
[Environment]::SetEnvironmentVariable("JAVA_HOME", "C:\Program Files\Java\jdk-17", "User")

# Verify Maven
mvn --version

# Should show correct Java version
```

### Issue 4: "Out of memory during build"

**Solution:**
```powershell
# Increase heap size
$env:MAVEN_OPTS = "-Xmx2048m"

# Or for javac
javac -J-Xmx2048m ...
```

---

## Best Practices

✅ **DO:**
- Use JDK 17+ for new projects (LTS version)
- Follow Java naming conventions (PascalCase for classes)
- Use Maven or Gradle for dependency management
- Write unit tests
- Use try-with-resources for file handling
- Follow SOLID principles
- Document public APIs with Javadoc
- Use meaningful variable names

❌ **DON'T:**
- Use outdated Java versions
- Ignore compiler warnings
- Use raw types (generics)
- Create mutable static fields
- Ignore exception handling
- Use reflection excessively
- Commit dependencies to git
- Hardcode configuration values

---

## Project Structure

```
kore-java-tools/
├── pom.xml
├── src/
│   ├── main/
│   │   ├── java/
│   │   │   └── com/kore/
│   │   │       ├── App.java
│   │   │       └── KoreProcessor.java
│   │   └── resources/
│   │       └── config.properties
│   └── test/
│       └── java/
│           └── com/kore/
│               └── AppTest.java
├── target/
│   ├── classes/
│   └── kore-java-tools-1.3.3.jar
└── README.md
```

---

## Quick Reference

```powershell
# Compilation
javac HelloWorld.java              # Compile single file
javac src/com/kore/*.java         # Compile directory
javac -d bin src/com/kore/*.java  # Compile to bin directory

# Execution
java HelloWorld                    # Run class (no .class)
java -cp .:lib/* com.kore.App     # Run with classpath

# Maven commands
mvn clean                          # Clean build directory
mvn compile                        # Compile source
mvn test                          # Run tests
mvn package                       # Create JAR
mvn install                       # Install locally
mvn deploy                        # Deploy to repository

# Useful commands
jar cvf app.jar -C bin .          # Create JAR
jar xf app.jar                    # Extract JAR
jps                               # List Java processes
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** Kotlin Setup & Integration Guide (coming next)
