# KORE FileFormat — Java

**Version 1.7.23** | [Maven Central](https://central.sonatype.com/artifact/com.github.arunkatherashala/kore-fileformat) | [GitHub](https://github.com/arunkatherashala/Kore)

High-performance columnar format with 11 ACID features. Pure Java implementation — reads and writes `.kore` binary files directly.

## Install

### Maven
```xml
<dependency>
  <groupId>com.github.arunkatherashala</groupId>
  <artifactId>kore-fileformat</artifactId>
  <version>1.7.23</version>
</dependency>
```

### Gradle
```groovy
implementation 'com.github.arunkatherashala:kore-fileformat:1.7.23'
```

## Quick Start

```java
import com.github.arunkatherashala.kore.*;
import java.util.*;

public class Example {
    public static void main(String[] args) throws Exception {
        // --- Write ---
        DataBlock block = new DataBlock();
        block.addColumn("price",    DataType.F64, Arrays.asList(10.5, 20.0, 30.75));
        block.addColumn("quantity", DataType.I64, Arrays.asList(100L, 200L, 300L));
        KoreWriter.writeFile("data.kore", block);

        // --- Read ---
        DataBlock result = KoreReader.fromFile("data.kore");
        System.out.printf("%d rows, %d columns%n", result.getNumRows(), result.getNumColumns());

        // --- CRC32 ---
        long checksum = Checksums.crc32("hello kore".getBytes());
        System.out.printf("crc32 = 0x%08x%n", checksum);  // 0x4b029b4b
    }
}
```

## API Reference

| Class | Method | Description |
|-------|--------|-------------|
| `KoreWriter` | `writeFile(path, block)` | Write DataBlock to .kore |
| `KoreReader` | `fromFile(path)` | Read .kore → DataBlock |
| `KoreReader` | `fromBytes(byte[])` | Deserialize from bytes |
| `Checksums` | `crc32(byte[])` | CRC32 checksum |
| `DataBlock` | `addColumn(name, type, data)` | Add column |

## Data Types

```java
DataType.F64       // double / Double
DataType.I64       // long / Long
DataType.STR       // String
DataType.STR_DICT  // dictionary-encoded String
DataType.BOOL      // boolean / Boolean
```

## Run Tests

```bash
mvn test
```
