# C# Setup & Integration Guide for KORE v1.3.3

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
| .NET Version | .NET 5.0 | .NET 7.0+ | Latest LTS recommended |
| C# Version | 9.0 | 12.0+ | C# language features |
| OS Support | Windows 10+ | Windows 11 | Also supports Linux/macOS |
| RAM | 2 GB | 4 GB | For compilation |
| Disk Space | 1 GB | 3 GB | .NET SDK + tools |

---

## Installation

### Step 1: Download .NET SDK

**Official Website:** https://dotnet.microsoft.com/download

**Windows Installation:**
```powershell
# Option 1: Manual download
# Go to https://dotnet.microsoft.com/download
# Download latest .NET SDK installer

# Option 2: Using Windows Package Manager
winget install Microsoft.DotNet.SDK.8

# Option 3: Using Chocolatey
choco install dotnet-sdk
```

### Step 2: Install .NET SDK

```powershell
# Run the downloaded installer
# Follow on-screen instructions
# Accept default locations

# Or install via package manager
winget install Microsoft.DotNet.SDK.8
```

### Step 3: Verify Installation

```powershell
# Check .NET version
dotnet --version

# List installed SDKs
dotnet --list-sdks

# List installed runtimes
dotnet --list-runtimes
```

---

## Verification

### Quick Check
```powershell
# Test .NET installation
dotnet --version

# Create test project
dotnet new console -n TestProject

# Navigate to test project
cd TestProject

# Run test
dotnet run

# Clean up
cd ..
Remove-Item TestProject -Recurse
```

### Complete Environment Check

```powershell
# Show .NET info
dotnet --info

# List all installed versions
dotnet --list-sdks
dotnet --list-runtimes

# Test compilation
dotnet new console -n Test && cd Test && dotnet build && cd .. && Remove-Item Test -Recurse
```

---

## KORE Integration

### C# with KORE

KORE is Rust-based but C# can be used for:
- Windows desktop tools
- Data analysis applications
- Integration tools
- Testing frameworks
- Administrative utilities

### Setup KORE C# Environment

**Step 1: Create .NET Project Structure**

```powershell
# Navigate to KORE directory
cd "c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"

# Create new C# project directory
New-Item -ItemType Directory -Name "csharp-tools"
cd csharp-tools

# Create new class library
dotnet new classlib -n KoreTools

# Create test project
dotnet new nunit -n KoreTools.Tests

# Create solution
dotnet new sln
dotnet sln add KoreTools/KoreTools.csproj
dotnet sln add KoreTools.Tests/KoreTools.Tests.csproj
```

**Step 2: Create Global.json for Consistent SDK**

```powershell
# Create global.json to pin .NET version
@{
  "sdk": {
    "version": "8.0.0",
    "rollForward": "latestMinor"
  }
} | ConvertTo-Json | Out-File global.json
```

**Step 3: Set Up Project File**

Edit `KoreTools/KoreTools.csproj`:
```xml
<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <LangVersion>latest</LangVersion>
    <Nullable>enable</Nullable>
    <Version>1.3.3</Version>
    <Authors>KORE Team</Authors>
    <Description>C# Tools for KORE v1.3.3</Description>
  </PropertyGroup>

  <ItemGroup>
    <!-- Add dependencies here -->
    <!-- Example: <PackageReference Include="Newtonsoft.Json" Version="13.0.3" /> -->
  </ItemGroup>

</Project>
```

---

## Common Tasks

### Building C# Projects

```powershell
# Build solution
dotnet build

# Build in Release mode
dotnet build --configuration Release

# Clean build
dotnet clean

# Rebuild
dotnet build --no-restore
```

### Running C# Programs

```powershell
# Run console application
dotnet run

# Run with arguments
dotnet run -- --arg1 value1 --arg2 value2

# Run specific project
dotnet run --project KoreTools.csproj
```

### Managing Dependencies

```powershell
# Add NuGet package
dotnet add package Newtonsoft.Json

# Add specific version
dotnet add package Newtonsoft.Json --version 13.0.3

# Update package
dotnet package update Newtonsoft.Json

# List packages
dotnet list package

# Remove package
dotnet remove package OldPackage
```

### Testing in C#

```powershell
# Run all tests
dotnet test

# Run specific test
dotnet test --filter "TestClassName"

# Run with verbose output
dotnet test --verbosity normal

# Generate test report
dotnet test --collect:"XPlat Code Coverage"
```

### Publishing Applications

```powershell
# Publish for Windows 64-bit
dotnet publish -c Release -r win-x64

# Publish self-contained
dotnet publish -c Release -r win-x64 --self-contained

# Publish framework-dependent
dotnet publish -c Release
```

---

## Troubleshooting

### Issue 1: ".NET SDK not found"

**Solution:**
```powershell
# Check installed SDKs
dotnet --list-sdks

# Download and install latest SDK
# https://dotnet.microsoft.com/download

# Or install via package manager
winget install Microsoft.DotNet.SDK.8
```

### Issue 2: "Target framework not installed"

**Solution:**
```powershell
# Install specific runtime
dotnet new globaljson --sdk-version 8.0.0 --roll-forward latestMinor

# Or change target framework in .csproj
# Change <TargetFramework>net8.0</TargetFramework>
# To compatible version
```

### Issue 3: "NuGet package not found"

**Solution:**
```powershell
# Clear NuGet cache
dotnet nuget locals all --clear

# Restore packages explicitly
dotnet restore

# Try adding package again
dotnet add package package-name
```

### Issue 4: "Compilation errors"

**Solution:**
```powershell
# Clean and rebuild
dotnet clean
dotnet build

# Check for deprecated APIs
dotnet build /p:EnforceCodeStyleInBuild=true

# Update .NET version if needed
dotnet sdk check
```

---

## Best Practices

✅ **DO:**
- Use latest LTS .NET version
- Enable nullable reference types
- Use async/await patterns
- Write unit tests
- Follow C# naming conventions (PascalCase for classes)
- Use dependency injection
- Keep global.json for consistency

❌ **DON'T:**
- Use outdated .NET frameworks
- Ignore compiler warnings
- Mix async and sync code
- Store secrets in code
- Ignore test failures
- Use dynamic typing excessively
- Skip null checks with nullable enabled

---

## C# Project Structure

```
kore-csharp/
├── global.json
├── KoreTools.sln
├── KoreTools/
│   ├── KoreTools.csproj
│   ├── Properties/
│   │   └── AssemblyInfo.cs
│   ├── Utilities/
│   │   ├── DataProcessor.cs
│   │   └── FileHandler.cs
│   └── Program.cs
├── KoreTools.Tests/
│   ├── KoreTools.Tests.csproj
│   ├── UtilitiesTests/
│   │   └── DataProcessorTests.cs
│   └── ...
└── README.md
```

---

## Advanced Features

### Creating NuGet Package

```powershell
# Add package metadata to .csproj
# Then pack
dotnet pack -c Release

# Publish to NuGet
dotnet nuget push bin/Release/KoreTools.1.3.3.nupkg \
  --api-key YOUR_API_KEY \
  --source https://api.nuget.org/v3/index.json
```

### Async/Await Pattern

```csharp
// Example: KORE data processing
public class KoreDataProcessor
{
    public async Task<bool> ProcessDataAsync(string filePath)
    {
        try
        {
            var data = await File.ReadAllTextAsync(filePath);
            // Process data
            return true;
        }
        catch (Exception ex)
        {
            Console.WriteLine($"Error: {ex.Message}");
            return false;
        }
    }
}
```

---

## Quick Reference

```powershell
# Project creation
dotnet new console              # Console app
dotnet new classlib             # Class library
dotnet new nunit                # NUnit test project
dotnet new sln                  # Solution file

# Building
dotnet build                    # Build project
dotnet clean                    # Clean build
dotnet publish -c Release       # Publish

# Running
dotnet run                      # Run application
dotnet test                     # Run tests

# Dependencies
dotnet add package <name>       # Add NuGet package
dotnet remove package <name>    # Remove package
dotnet list package             # List packages

# Version
dotnet --version                # Show .NET version
dotnet --info                   # Detailed info
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** Rust Setup & Integration Guide (coming next)
