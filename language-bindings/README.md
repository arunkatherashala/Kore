# Phase 6: Language Bindings

**Status:** 🚀 In Progress  
**Timeline:** 2-4 weeks  
**Target:** Support for Go, Java, JavaScript, .NET, Ruby, PHP  

## Overview

Multi-language support enables:
- ✅ Go (CGO bindings)
- ✅ Java (JNI bindings)
- ✅ JavaScript/Node.js (NAPI)
- ✅ C# / .NET (P/Invoke)
- ✅ Ruby (FFI)
- ✅ PHP (Extension)

## Languages Priority

### Tier 1 (High Priority)
1. **Go** — Popular in data engineering
2. **Java** — JVM ecosystem (Spark, Hadoop)
3. **JavaScript** — Node.js + web

### Tier 2 (Medium Priority)
4. **C# / .NET** — Enterprise
5. **Ruby** — Data science
6. **PHP** — Web applications

### Tier 3 (Low Priority)
7. **C** — Direct FFI
8. **C++** — System integration
9. **R** — Statistical analysis

## Go Bindings

```go
package kore

import (
    kore "github.com/kore/go-kore"
)

func main() {
    // Read
    data, err := kore.ReadKore("data.kore")
    
    // Write
    err = kore.WriteKore("output.kore", schema, data)
    
    // Column read
    column, err := kore.ReadColumn("data.kore", "name")
    
    // Stats
    stats, err := kore.GetStats("data.kore")
}
```

## Java Bindings

```java
import io.kore.KoreReader;
import io.kore.KoreWriter;

public class Main {
    public static void main(String[] args) {
        // Read
        var data = KoreReader.read("data.kore");
        
        // Write
        KoreWriter.write("output.kore", schema, data);
        
        // Column read
        var column = KoreReader.readColumn("data.kore", "name");
    }
}
```

## JavaScript/TypeScript Bindings

```typescript
import { Kore } from 'kore';

// Read
const data = await Kore.read('data.kore');

// Write
await Kore.write('output.kore', data);

// Column read
const column = await Kore.readColumn('data.kore', 'name');
```

## Implementation Phases

### Phase 6A: Go (Week 1)
- [ ] CGO wrapper
- [ ] Error handling
- [ ] Type conversion
- [ ] Testing

### Phase 6B: Java (Week 2)
- [ ] JNI wrapper
- [ ] JVM integration
- [ ] Maven package
- [ ] Testing

### Phase 6C: JavaScript (Week 2)
- [ ] NAPI bindings
- [ ] npm package
- [ ] TypeScript types
- [ ] Testing

### Phase 6D: Others (Week 3-4)
- [ ] .NET P/Invoke
- [ ] Ruby FFI
- [ ] PHP extension

## Dependencies

| Language | Tool | Version |
|----------|------|---------|
| Go | cgo | 1.19+ |
| Java | JNI | Java 8+ |
| Node.js | NAPI | 14+ |
| .NET | P/Invoke | 6+ |
| Ruby | FFI | 3.0+ |
| PHP | Extension | 8.0+ |

## Roadmap

- [ ] Go bindings (CGO)
- [ ] Java bindings (JNI)
- [ ] JavaScript bindings (NAPI)
- [ ] .NET bindings (P/Invoke)
- [ ] Ruby bindings (FFI)
- [ ] PHP extension
- [ ] C++ bindings
- [ ] R bindings
- [ ] Documentation
- [ ] Package distribution

## Testing

```bash
# Go
go test ./...

# Java
mvn test

# JavaScript
npm test

# .NET
dotnet test

# Ruby
bundle exec rspec

# PHP
phpunit tests/
```

## Known Limitations

- Requires language compiler/SDK
- Cross-platform compatibility needed
- Performance varies by language

## Contributors

Assigned for Phase 6 development.

---

**Next:** Begin with Go and Java bindings.
