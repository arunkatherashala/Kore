# Swift Setup & Integration Guide for KORE v1.3.3

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
| Swift Version | 5.7+ | 5.9+ | Latest stable |
| macOS Version | 11+ | 13+ | Monterey or newer |
| Xcode | 13.0+ | 15.0+ | From App Store |
| SPM | Included | Included | Swift Package Manager |
| OS Support | macOS only | macOS 13+ | iOS/watchOS optional |
| RAM | 2 GB | 4 GB | For compilation |
| Disk Space | 2 GB | 5 GB | Swift + SDKs |

---

## Installation

### Step 1: Install Xcode

**macOS:**
```bash
# Install from App Store (recommended)
# Or via command line:
xcode-select --install

# Verify installation
xcode-select -p
# Expected: /Applications/Xcode.app/Contents/Developer

# Accept license
sudo xcodebuild -license accept
```

### Step 2: Verify Swift Installation

```bash
# Check Swift version
swift --version

# Expected:
# Swift version 5.9.x (swift-5.9.x-release)
# Target: arm64-apple-macosx13.0

# Check available toolchain
xcrun swift --version
```

### Step 3: Setup KORE Swift Project

```bash
# Create directory
mkdir kore-swift-tools
cd kore-swift-tools

# Initialize Swift package
swift package init --type executable

# Or create Package.swift
cat > Package.swift << 'EOF'
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "kore-swift-tools",
    platforms: [
        .macOS(.v13)
    ],
    dependencies: [
        .package(url: "https://github.com/apple/swift-argument-parser.git", from: "1.2.0"),
        .package(url: "https://github.com/apple/swift-nio.git", from: "2.0.0")
    ],
    targets: [
        .executableTarget(
            name: "kore",
            dependencies: [
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
                .product(name: "NIO", package: "swift-nio")
            ]
        ),
        .testTarget(
            name: "koreTests",
            dependencies: ["kore"]
        )
    ]
)
EOF

# Build project
swift build
```

---

## Verification

### Quick Check

```bash
# Check Swift version
swift --version

# Create test file
cat > test.swift << 'EOF'
import Foundation

print("Hello from KORE Swift!")
print("Swift version: \(ProcessInfo.processInfo.operatingSystemVersionString)")

struct KoreInfo {
    let version = "1.3.3"
    let name = "KORE"
}

let info = KoreInfo()
print("KORE Version: \(info.version)")
EOF

# Run
swift test.swift

# Clean up
rm test.swift
```

### Complete Environment

```bash
# Show Swift compiler info
swiftc --version

# List installed SDKs
xcrun --show-sdk-path

# Show toolchain
swift --version
```

---

## KORE Integration

### Swift with KORE

Swift is excellent for:
- macOS applications
- iOS/iPadOS apps
- Command-line tools
- Server-side applications
- High-performance processing

### Create KORE Swift Library

**Sources/kore/KoreProcessor.swift:**
```swift
import Foundation

public class KoreProcessor {
    public static let version = "1.3.3"
    
    private let filePath: String
    
    public init(filePath: String) {
        self.filePath = filePath
    }
    
    public struct KoreMetadata: Codable {
        let filename: String
        let version: String
        let timestamp: Int
        let status: String
    }
    
    public func process() -> KoreMetadata {
        return KoreMetadata(
            filename: URL(fileURLWithPath: filePath).lastPathComponent,
            version: Self.version,
            timestamp: Int(Date().timeIntervalSince1970),
            status: "processed"
        )
    }
    
    public func getMetadataJson() -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        
        let metadata = process()
        if let jsonData = try? encoder.encode(metadata),
           let jsonString = String(data: jsonData, encoding: .utf8) {
            return jsonString
        }
        return "{}"
    }
}
```

**Sources/kore/main.swift:**
```swift
import Foundation

print("KORE Swift Integration v\(KoreProcessor.version)")

let processor = KoreProcessor(filePath: "data.kore")
print(processor.getMetadataJson())
```

---

## Common Tasks

### Building Swift Programs

```bash
# Build project
swift build

# Build with optimizations
swift build -c release

# Create executable
swift build -c release

# Run executable
./.build/release/kore

# Clean build
swift package clean
```

### Running Swift Programs

```bash
# Run main executable
swift run

# Run with arguments
swift run kore --file data.kore

# Run tests
swift test

# Run specific test
swift test --filter KoreProcessorTests
```

### Swift KORE Integration Examples

**Read KORE Files:**
```swift
struct KoreReader {
    let filePath: String
    
    func readMetadata() -> [String: Any]? {
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: filePath)) else {
            return nil
        }
        
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }
    
    func validate() -> Bool {
        return FileManager.default.fileExists(atPath: filePath) &&
               FileManager.default.isReadableFile(atPath: filePath)
    }
}

let reader = KoreReader(filePath: "data.kore")
print("Valid: \(reader.validate())")
```

**HTTP API with Swift NIO:**
```swift
import NIO
import NIOHTTP1

class KoreAPIHandler: ChannelInboundHandler {
    typealias InboundIn = HTTPServerRequestPart
    typealias OutboundOut = HTTPServerResponsePart
    
    private let processor = KoreProcessor(filePath: "data.kore")
    
    func channelRead(context: ChannelHandlerContext, data: NIOAny) {
        let requestPart = unwrapInboundIn(data)
        
        switch requestPart {
        case .head(let request):
            if request.uri == "/api/kore/metadata" {
                let response = HTTPServerResponsePart.head(
                    HTTPResponseHead(version: request.version, status: .ok)
                )
                context.write(wrapOutboundOut(response), promise: nil)
                
                let body = HTTPServerResponsePart.body(
                    .byteBuffer(ByteBuffer(string: processor.getMetadataJson()))
                )
                context.write(wrapOutboundOut(body), promise: nil)
                
                let end = HTTPServerResponsePart.end(nil)
                context.writeAndFlush(wrapOutboundOut(end), promise: nil)
            }
        default:
            break
        }
    }
}
```

---

## Troubleshooting

### Issue 1: "swift command not found"

**Solution:**
```bash
# Check Xcode installation
xcode-select -p

# If not installed, install Xcode Command Line Tools
xcode-select --install

# Accept license
sudo xcodebuild -license accept
```

### Issue 2: "Cannot build for target architecture"

**Solution:**
```bash
# Check current architecture
swift -version

# Build for specific architecture
swift build -Xswiftc -target -Xswiftc arm64-apple-macosx13.0

# Or use Rosetta 2 (Intel compatibility on Apple Silicon)
arch -x86_64 swift build
```

### Issue 3: "Module not found"

**Solution:**
```bash
# Update dependencies
swift package update

# Clean and rebuild
swift package clean
swift build

# Check Package.swift dependencies
cat Package.swift
```

---

## Best Practices

✅ **DO:**
- Use Swift Package Manager (SPM)
- Write Swift with proper naming conventions (camelCase)
- Use protocols for abstractions
- Implement Error types for error handling
- Use struct for simple data models
- Write tests with XCTest
- Use async/await for concurrency
- Document public APIs with doc comments

❌ **DON'T:**
- Use force unwrap (!)
- Mix Swift versions
- Hardcode paths
- Ignore compiler warnings
- Use deprecated APIs
- Create global state
- Mix protocols without clear purpose
- Ignore memory management

---

## Project Structure

```
kore-swift-tools/
├── Package.swift
├── Package.resolved
├── README.md
├── Sources/
│   └── kore/
│       ├── main.swift
│       └── KoreProcessor.swift
├── Tests/
│   └── koreTests/
│       └── KoreProcessorTests.swift
└── .build/
    └── release/
        └── kore
```

---

## Quick Reference

```bash
# Swift commands
swift --version                # Check version
swift build                   # Build project
swift build -c release        # Release build
swift run                     # Run executable
swift test                    # Run tests
swift package describe        # Show package info

# Package management
swift package init            # Initialize new package
swift package init --type lib # Library package
swift package update          # Update dependencies
swift package clean           # Clean build
swift package reset           # Reset cache
```

---

## macOS/iOS Integration

**For macOS app:**
```swift
import Cocoa

class KoreApp: NSApplication {
    override func applicationDidFinishLaunching(_ notification: Notification) {
        let processor = KoreProcessor(filePath: "data.kore")
        print(processor.getMetadataJson())
    }
}
```

**For iOS:**
```swift
import UIKit

class KoreViewController: UIViewController {
    let processor = KoreProcessor(filePath: "data.kore")
    
    override func viewDidLoad() {
        super.viewDidLoad()
        print(processor.getMetadataJson())
    }
}
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-06-03 | Initial Swift setup guide for KORE v1.3.3 |

---

**Status: ✅ Production Ready**

**Next:** Clojure Setup & Integration Guide
