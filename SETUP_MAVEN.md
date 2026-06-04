# Maven Setup & Integration Guide for KORE v1.3.3

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
| Java Version | JDK 8 | JDK 11+ | Maven requires Java installed |
| Maven Version | 3.6.0 | 3.9+ | Latest stable recommended |
| OS Support | Windows 10+ | Windows 10, 11 | Also supports Linux/macOS |
| RAM | 2 GB | 4 GB | For build operations |
| Disk Space | 500 MB | 1 GB | For Maven + dependencies |

---

## Installation

### Step 1: Install Java (Prerequisite)

**Check if Java is installed:**
```powershell
java -version
```

**If not installed, download from:**
https://www.oracle.com/java/technologies/downloads/

Or use Windows Package Manager:
```powershell
# Install JDK 11+
winget install Oracle.JDK.11
# or
winget install Eclipse.Temurin
```

### Step 2: Download Maven

**Official Website:** https://maven.apache.org/download.cgi

**Download Latest Version:**
```powershell
# Option 1: Manual download
# Go to https://maven.apache.org/download.cgi
# Download "apache-maven-3.9.x-bin.zip"

# Option 2: Using PowerShell
$MavenUrl = "https://dlcdn.apache.org/maven/maven-3/3.9.3/binaries/apache-maven-3.9.3-bin.zip"
$DownloadPath = "$env:USERPROFILE\Downloads\apache-maven-3.9.3-bin.zip"
Invoke-WebRequest -Uri $MavenUrl -OutFile $DownloadPath
```

### Step 3: Install Maven

**Windows Installation:**

```powershell
# Create Maven installation directory
New-Item -ItemType Directory -Force -Path "C:\tools\maven"

# Extract downloaded Maven
Expand-Archive -Path "$env:USERPROFILE\Downloads\apache-maven-3.9.3-bin.zip" `
               -DestinationPath "C:\tools\maven"

# Rename for easier path
Rename-Item "C:\tools\maven\apache-maven-3.9.3" "C:\tools\maven\maven-3.9.3"
```

### Step 4: Configure System Environment Variables

**Windows GUI Method:**
1. Open "Edit environment variables for your account"
2. Click "New" to add new variable:
   - Variable name: `MAVEN_HOME`
   - Variable value: `C:\tools\maven\maven-3.9.3`
3. Find `PATH` variable and add: `;%MAVEN_HOME%\bin`
4. Click OK and restart PowerShell

**PowerShell Method:**
```powershell
# Set MAVEN_HOME
[Environment]::SetEnvironmentVariable("MAVEN_HOME", "C:\tools\maven\maven-3.9.3", "User")

# Add to PATH
$currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
$newPath = $currentPath + ";C:\tools\maven\maven-3.9.3\bin"
[Environment]::SetEnvironmentVariable("PATH", $newPath, "User")

# Restart PowerShell to apply changes
```

---

## Verification

### Quick Check
```powershell
# Test Maven installation
mvn --version

# Test Java installation
java -version

# Test Javac (Java compiler)
javac -version
```

**Expected Output:**
```
Apache Maven 3.9.3 (...java version: 11.0.x or higher...

java version "11.0.x" (...) 2023-...
Java(TM) SE Runtime Environment (...)

javac 11.0.x
```

### Complete Environment Check
```powershell
# Show Maven home
echo $env:MAVEN_HOME

# Show Java home
echo $env:JAVA_HOME

# Test Maven with help command
mvn help:active-profiles

# Check Maven repository location
mvn help:evaluate -Dexpression=settings.localRepository
```

---

## KORE Integration

### KORE with Maven

**Note:** KORE is a Rust project and uses Cargo for building, but Maven can be used for:
- Building Java components
- Managing Java dependencies
- Running Java benchmarks
- Integration with Java-based tools

### Setup KORE Maven Environment

**Step 1: Create Maven Settings (Optional)**

Create `~/.m2/settings.xml` for custom configuration:

```xml
<settings xmlns="http://maven.apache.org/SETTINGS/1.0.0"
  xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
  xsi:schemaLocation="http://maven.apache.org/SETTINGS/1.0.0
                      http://maven.apache.org/xsd/settings-1.0.0.xsd">
  
  <!-- Local Maven repository -->
  <localRepository>C:/Users/YourName/.m2/repository</localRepository>
  
  <!-- Offline mode (optional) -->
  <!-- <offline>false</offline> -->
  
</settings>
```

**Step 2: Configure Maven for KORE Project**

If KORE needs Java components, create `pom.xml` in project root:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<project>
  <modelVersion>4.0.0</modelVersion>
  
  <groupId>com.kore</groupId>
  <artifactId>kore-java-tools</artifactId>
  <version>1.3.3</version>
  <packaging>jar</packaging>
  
  <name>KORE Java Tools</name>
  <description>Java integration tools for KORE v1.3.3</description>
  
  <properties>
    <maven.compiler.source>11</maven.compiler.source>
    <maven.compiler.target>11</maven.compiler.target>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  
  <dependencies>
    <!-- JUnit for testing -->
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
          <source>11</source>
          <target>11</target>
        </configuration>
      </plugin>
    </plugins>
  </build>
</project>
```

---

## Common Tasks

### Building Java Projects with Maven

```powershell
# Clean and build project
mvn clean package

# Build without running tests
mvn clean package -DskipTests

# Run all tests
mvn test

# Build and install to local repository
mvn clean install

# Generate documentation (Javadoc)
mvn javadoc:javadoc
```

### Dependency Management

```powershell
# Add a dependency
mvn org.apache.maven.plugins:maven-dependency-plugin:3.5.0:add

# View dependency tree
mvn dependency:tree

# Check for outdated dependencies
mvn versions:display-dependency-updates

# Update all dependencies
mvn versions:use-latest-versions
```

### Running Java Tests

```powershell
# Run all tests
mvn test

# Run specific test class
mvn test -Dtest=TestClassName

# Run with test report
mvn test surefire-report:report

# View test results
# Reports generated in: target/site/surefire-report.html
```

### Creating JAR Files

```powershell
# Build JAR (includes tests)
mvn clean package

# Build without tests
mvn clean package -DskipTests

# Build executable JAR
mvn clean compile assembly:single
```

---

## Troubleshooting

### Issue 1: "Maven is not recognized"

**Solution:**
```powershell
# Verify Maven is in PATH
where mvn

# If not found:
# 1. Check MAVEN_HOME environment variable
echo $env:MAVEN_HOME

# 2. Restart PowerShell or IDE
# 3. Reinstall Maven with proper PATH setup
```

### Issue 2: "JAVA_HOME is not set"

**Solution:**
```powershell
# Set JAVA_HOME
[Environment]::SetEnvironmentVariable("JAVA_HOME", "C:\Program Files\Java\jdk-11", "User")

# Verify
echo $env:JAVA_HOME

# Restart PowerShell
```

### Issue 3: Slow Maven downloads

**Solution:**
```powershell
# Use Maven central mirror (edit settings.xml)
# Add to <mirrors> section:
<mirror>
  <id>aliyun</id>
  <mirrorOf>central</mirrorOf>
  <name>Aliyun Maven Mirror</name>
  <url>https://maven.aliyun.com/repository/public</url>
</mirror>
```

### Issue 4: "OutOfMemoryError" during build

**Solution:**
```powershell
# Increase Maven memory
$env:MAVEN_OPTS = "-Xmx2048m"

# Run build
mvn clean package
```

### Issue 5: Dependency conflicts

**Solution:**
```powershell
# View dependency tree to find conflicts
mvn dependency:tree

# Exclude conflicting dependencies in pom.xml
<dependency>
  <groupId>...</groupId>
  <artifactId>...</artifactId>
  <exclusions>
    <exclusion>
      <groupId>conflicting.group</groupId>
      <artifactId>conflicting-artifact</artifactId>
    </exclusion>
  </exclusions>
</dependency>
```

---

## Best Practices

✅ **DO:**
- Use consistent Maven versions across team
- Pin dependency versions explicitly
- Use Maven Central Repository
- Run tests before packaging
- Keep pom.xml organized
- Use profiles for different environments
- Document all custom plugins

❌ **DON'T:**
- Use SNAPSHOT versions in production
- Build without running tests
- Ignore Maven warnings
- Use very old Maven versions
- Store passwords in settings.xml
- Ignore dependency conflicts

---

## Maven Configuration Checklist

Before using Maven with KORE:

- [ ] Java installed (version 8+)
- [ ] `java -version` works
- [ ] Maven downloaded and installed
- [ ] `mvn --version` works
- [ ] MAVEN_HOME environment variable set
- [ ] Maven in PATH environment variable
- [ ] JAVA_HOME environment variable set
- [ ] pom.xml created (if using Maven projects)
- [ ] Local Maven repository working

---

## Advanced Configuration

### Maven Profiles for KORE Environments

**In pom.xml:**
```xml
<profiles>
  <profile>
    <id>development</id>
    <properties>
      <maven.test.skip>false</maven.test.skip>
      <build.timestamp>${maven.build.timestamp}</build.timestamp>
    </properties>
  </profile>
  
  <profile>
    <id>production</id>
    <properties>
      <maven.test.skip>false</maven.test.skip>
      <build.optimization>true</build.optimization>
    </properties>
  </profile>
</profiles>
```

**Usage:**
```powershell
# Build with development profile
mvn clean package -P development

# Build with production profile
mvn clean package -P production
```

### Multi-Module Projects

For large KORE Java components:

```
kore-parent/
├── pom.xml (parent)
├── kore-core/
│   └── pom.xml
├── kore-utils/
│   └── pom.xml
└── kore-tests/
    └── pom.xml
```

**Parent pom.xml:**
```xml
<modules>
  <module>kore-core</module>
  <module>kore-utils</module>
  <module>kore-tests</module>
</modules>
```

---

## Additional Resources

| Resource | Link | Purpose |
|----------|------|---------|
| Maven Official | https://maven.apache.org/ | Official documentation |
| Maven Central | https://mvnrepository.com/ | Dependency search |
| POM Reference | https://maven.apache.org/pom.html | POM structure |
| Plugins | https://maven.apache.org/plugins/ | Available plugins |
| Settings | https://maven.apache.org/settings.html | Configuration reference |

---

## Quick Reference Commands

```powershell
# Project lifecycle
mvn clean                      # Remove build directory
mvn compile                    # Compile source code
mvn test                       # Run tests
mvn package                    # Create JAR/WAR
mvn install                    # Install to local repo
mvn deploy                     # Deploy to remote repo

# Common combinations
mvn clean package              # Full build cycle
mvn clean install              # Build and install
mvn clean package -DskipTests  # Skip tests

# Information
mvn --version                  # Maven version
mvn help:active-profiles       # Show active profiles
mvn help:describe              # Describe plugin
mvn dependency:tree            # Dependency tree
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** C# Setup & Integration Guide (coming next)
