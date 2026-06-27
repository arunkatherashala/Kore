# Build Instructions

## Prerequisites

- Java 11+
- Maven 3.8+
- Scala 2.12.17
- Spark 3.4.1 (provided scope, not needed for build)

## Build Steps

### 1. Build the JAR

```bash
cd spark-kore
mvn clean package -DskipTests
```

Output: `target/spark-kore-1.0.0.jar`

### 2. Run Tests

```bash
mvn test
```

Expected output: All 20 tests passing ✅

### 3. Install Locally

```bash
mvn install -DskipTests
```

This installs the JAR to your local Maven repository (~/.m2/repository)

## Using in Your Spark Application

### Maven POM.xml

```xml
<dependency>
    <groupId>org.kore</groupId>
    <artifactId>spark-kore</artifactId>
    <version>1.0.0</version>
</dependency>
```

### Spark Submit

```bash
spark-submit \
  --packages org.kore:spark-kore:1.0.0 \
  --class org.kore.spark.examples.BasicExample \
  examples/BasicExample.scala
```

Or with JAR file:

```bash
spark-submit \
  --jars spark-kore/target/spark-kore-1.0.0.jar \
  --class org.kore.spark.examples.BasicExample \
  examples/BasicExample.scala
```

### Scala REPL

```bash
spark-shell \
  --jars spark-kore/target/spark-kore-1.0.0.jar

scala> spark.read.format("kore").load("data.kore").show()
```

### Python PySpark

```bash
pyspark \
  --jars spark-kore/target/spark-kore-1.0.0.jar

>>> spark.read.format("kore").load("data.kore").show()
```

## Running Examples

### Example 1: Basic Read/Write

```bash
mvn clean package -DskipTests
spark-submit \
  --class org.kore.spark.examples.BasicExample \
  target/spark-kore-1.0.0.jar
```

### Example 2: Filter Pushdown

```bash
spark-submit \
  --class org.kore.spark.examples.FilterPushdownExample \
  target/spark-kore-1.0.0.jar
```

### Example 3: Compression

```bash
spark-submit \
  --class org.kore.spark.examples.CompressionExample \
  target/spark-kore-1.0.0.jar
```

## Troubleshooting

### Problem: Build fails with "Cannot find symbol"

**Solution**: Update Scala compiler
```bash
mvn clean compile -DskipTests
```

### Problem: Tests fail with "DataSourceV2 not found"

**Solution**: Ensure Spark 3.4.1 is in classpath
```bash
mvn test -Dspark.version=3.4.1
```

### Problem: JAR too large

**Solution**: Use shade plugin (already configured)
```bash
mvn package -DskipTests  # Creates fat JAR
```

## Testing

### Run specific test

```bash
mvn test -Dtest=KoreDataSourceTest#testName
```

### Run with debug output

```bash
mvn test -X
```

### Run with coverage (if jacoco configured)

```bash
mvn test jacoco:report
# Report at: target/site/jacoco/index.html
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Build Spark Connector

on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions/setup-java@v2
        with:
          java-version: '11'
      - name: Build
        run: cd spark-kore && mvn clean package
```

## Performance Optimization

### JVM Tuning for Build

```bash
export MAVEN_OPTS="-Xmx2G -XX:+UseG1GC"
mvn clean package -DskipTests
```

### Parallel Test Execution

```bash
mvn test -T 4  # Run 4 tests in parallel
```

## Publishing

### Local Maven Repository

```bash
mvn install
```

Installed to: `~/.m2/repository/org/kore/spark-kore/1.0.0/`

### Maven Central (future)

```bash
mvn clean package
mvn deploy -DrepositoryId=central-portal
```

## Versioning

Update version in pom.xml:

```xml
<version>1.0.1</version>  <!-- Change here -->
```

Then rebuild:

```bash
mvn clean package -DskipTests
```

## Additional Resources

- [Spark DataSource API Documentation](https://spark.apache.org/docs/latest/sql-data-sources-custom.html)
- [Spark SQL Guide](https://spark.apache.org/docs/latest/sql-programming-guide.html)
- [Scala Build Tool](https://www.scala-sbt.org/)

