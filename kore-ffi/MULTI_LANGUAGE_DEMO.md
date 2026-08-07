# KORE Multi-Language Demo

This document shows how to use the KORE in-memory query engine from seven
different languages.  All examples assume you have built the native library:

```bash
cargo build --release -p kore-ffi
```

The compiled library is at:

| OS      | Path                             |
|---------|----------------------------------|
| Windows | `target/release/kore_ffi.dll`    |
| Linux   | `target/release/libkore_ffi.so`  |
| macOS   | `target/release/libkore_ffi.dylib` |

---

## File index

| Language | File                                              | Mode        |
|----------|---------------------------------------------------|-------------|
| Python   | `bindings/python/kore.py`                         | ctypes      |
| Java     | `bindings/java/com/kore/KoreEngine.java`           | REST client |
| Node.js  | `bindings/nodejs/kore.js`                         | ffi-napi / REST |
| Go       | `bindings/go/kore.go`                             | CGo         |
| C#       | `bindings/csharp/KoreEngine.cs`                   | P/Invoke    |
| Ruby     | `bindings/ruby/kore.rb`                           | Fiddle      |
| PHP      | `bindings/php/kore.php`                           | PHP FFI     |

---

## 1. Python

**Requirements**: Python 3.9, 3.10, 3.11, 3.12, 3.13 (all active releases).

```bash
# Optional: override library path
export KORE_LIB=target/release/libkore_ffi.so

python bindings/python/kore.py   # smoke test
```

```python
from kore import KoreBlock, KoreModel, KoreSession, ModelType

# DataBlock + ML
block = KoreBlock()
block.add_f64("x", [1.0, 2.0, 3.0])
block.add_i64("id", [1, 2, 3])

model = KoreModel(ModelType.LINEAR_REGRESSOR)
model.fit([[1.0], [2.0], [3.0]], [2.0, 4.0, 6.0])
print(model.predict([[4.0], [5.0]]))   # [8.0, 10.0]

# SQL Session
with KoreSession() as sess:
    sess.load_table("sales", [
        {"region": "North", "amount": 1000.0},
        {"region": "South", "amount": 2000.0},
    ])
    print(sess.row_count("sales"))   # 2
    print(sess.query("SELECT SUM(amount) AS total FROM sales"))
    # [{"total": 3000.0}]

    # Use a DataBlock as a SQL table
    sess.register_block("blk", block)
    print(sess.query("SELECT * FROM blk"))
```

---

## 2. Java (REST API client)

**Requirements**: Java 11 LTS, 17 LTS, 21 LTS, 25 LTS — all LTS releases supported.

```bash
# Terminal 1: start the API server
cargo run --release -p kore-api

# Terminal 2: compile and run
javac -d out bindings/java/com/kore/KoreEngine.java
java -cp out com.kore.KoreClient
```

```java
import com.kore.KoreClient;

try (var kore = new KoreClient()) {           // connects to localhost:3000
    kore.loadTable("nums", List.of(
        Map.of("id", 1, "val", 10.0),
        Map.of("id", 2, "val", 20.0)
    ));
    System.out.println(kore.rowCount("nums"));      // 2
    System.out.println(kore.query("SELECT SUM(val) AS s FROM nums"));
    // [{s=30.0}]

    // ML
    var X = List.of(List.of(1.0), List.of(2.0), List.of(3.0));
    var y = List.of(2.0, 4.0, 6.0);
    String modelId = kore.fit(KoreClient.LINEAR_REGRESSOR, 0, 0, X, y);
    System.out.println(kore.predict(modelId, List.of(List.of(4.0))));
    // [8.0]
}
```

> **Server URL**: set `KORE_API_URL=http://host:port` to point to a remote
> instance.

---

## 3. Node.js

**Requirements**: Node.js 18 LTS, 20 LTS, 22 LTS, 24 (current) — all active releases.  
Native mode (faster): `npm install ffi-napi ref-napi`  
REST mode (no install): start `cargo run --release -p kore-api`

```bash
node bindings/nodejs/kore.js data.csv   # demo with optional CSV arg
```

```javascript
const { KoreSession, KoreBlock, ModelType, USE_NATIVE } = require('./kore');

console.log('Native mode:', USE_NATIVE);

const sess = new KoreSession();

await sess.loadTable('products', [
    { id: 1, price: 9.99 },
    { id: 2, price: 24.99 },
]);
console.log(await sess.rowCount('products'));   // 2
console.log(await sess.query('SELECT SUM(price) AS total FROM products'));
// [ { total: 34.98 } ]

await sess.loadCsv('sales', '/data/sales.csv');
const top = await sess.query('SELECT * FROM sales ORDER BY amount DESC LIMIT 5');
console.log(top);

await sess.close();
```

---

## 4. Go

**Requirements**: Go 1.21, 1.22, 1.23, 1.24 (all supported releases), CGo, GCC/Clang.

```bash
export CGO_LDFLAGS="-L$(pwd)/target/release -lkore_ffi -Wl,-rpath,$(pwd)/target/release"
export CGO_CFLAGS="-I$(pwd)/kore-ffi/include"
go build ./bindings/go/...
```

```go
package main

import (
    "fmt"
    "log"
    kore "github.com/yourorg/kore/bindings/go"
)

func main() {
    // DataBlock
    blk := kore.NewBlock()
    defer blk.Free()
    blk.AddF64("x", []float64{1, 2, 3, 4})
    blk.AddI64("id", []int64{10, 20, 30, 40})
    fmt.Println(blk.NumRows(), blk.NumCols())   // 4 2

    // ML
    model, _ := kore.NewModel(kore.LinearRegressor, 0, 0)
    defer model.Free()
    model.Fit([]float64{1, 2, 3}, 3, 1, []float64{2, 4, 6})
    preds, _ := model.Predict([]float64{4, 5}, 2, 1)
    fmt.Println(preds)   // [8 10]

    // SQL Session
    sess, err := kore.NewSession()
    if err != nil { log.Fatal(err) }
    defer sess.Close()

    if err := sess.RegisterBlock("blk", blk); err != nil { log.Fatal(err) }
    rows, _ := sess.Query("SELECT SUM(x) AS s FROM blk")
    fmt.Println(rows)   // [map[s:10]]

    if err := sess.LoadCSV("sales", "/data/sales.csv"); err != nil { log.Fatal(err) }
    n, _ := sess.RowCount("sales")
    fmt.Println("sales rows:", n)
}
```

---

## 5. C#

**Requirements**: .NET 8 LTS, .NET 9, .NET 10 LTS — all active releases.

```bash
# Add kore_ffi.dll / libkore_ffi.so to your project output directory, then:
dotnet run
```

```csharp
using Kore;

// DataBlock
using var block = new KoreBlock();
block.AddF64("x", new[] { 1.0, 2.0, 3.0 });
Console.WriteLine(block);   // KoreBlock(rows=3, cols=1)

// ML
using var model = new KoreModel(ModelType.LinearRegressor);
model.Fit(new[] { 1.0, 2.0, 3.0 }, 3, 1, new[] { 2.0, 4.0, 6.0 });
var preds = model.Predict(new[] { 4.0, 5.0 }, 2, 1);
Console.WriteLine(string.Join(", ", preds));   // 8, 10

// SQL Session
using var sess = new KoreSession();
sess.LoadTable("products", new List<Dictionary<string, object?>>
{
    new() { ["name"] = "Widget", ["price"] = 9.99 },
    new() { ["name"] = "Gadget", ["price"] = 24.99 },
});
Console.WriteLine(sess.RowCount("products"));   // 2
var rows = sess.Query("SELECT name, price FROM products ORDER BY price DESC");
foreach (var r in rows) Console.WriteLine(r["name"] + " = " + r["price"]);

// register_block -> SQL
sess.RegisterBlock("blk", block);
var agg = sess.Query("SELECT SUM(x) AS total FROM blk");
Console.WriteLine(agg[0]["total"]);   // 6
```

---

## 6. Ruby

**Requirements**: Ruby 3.1, 3.2, 3.3, 3.4 — all active releases, Fiddle (stdlib, no gems).

```bash
ruby bindings/ruby/kore.rb   # smoke test
```

```ruby
require_relative 'kore'

# DataBlock
blk = Kore::Block.new
blk.add_f64('x', [1.0, 2.0, 3.0])
blk.add_i64('id', [10, 20, 30])
puts blk                   # KoreBlock(rows=3, cols=2)
puts blk.get_f64('x').inspect   # [1.0, 2.0, 3.0]

# ML
model = Kore::Model.new(Kore::Model::LINEAR_REGRESSOR)
model.fit([1.0, 2.0, 3.0], 3, 1, [2.0, 4.0, 6.0])
puts model.predict([4.0, 5.0], 2, 1).inspect   # [8.0, 10.0]

# SQL Session
sess = Kore::Session.new
sess.load_table('sales', [
  { region: 'North', amount: 1000 },
  { region: 'South', amount: 2000 },
])
puts sess.row_count('sales')   # 2
puts sess.query('SELECT SUM(amount) AS total FROM sales').inspect
# [{"total"=>3000}]

sess.register_block('blk', blk)
puts sess.query('SELECT * FROM blk').inspect
sess.close
```

---

## 7. PHP

**Requirements**: PHP 8.0, 8.1, 8.2, 8.3, 8.4 — all active releases, `ext-ffi` enabled.

```ini
; php.ini
extension=ffi
ffi.enable=true
```

```bash
php bindings/php/kore.php   # smoke test
```

```php
<?php
require_once 'kore.php';
use Kore\{Block, Model, Session, ModelType};

// DataBlock
$blk = new Block();
$blk->addF64('x', [1.0, 2.0, 3.0]);
echo $blk . "\n";   // KoreBlock(rows=3, cols=1)

// ML
$model = new Model(ModelType::LINEAR_REGRESSOR);
$model->fit([1.0, 2.0, 3.0], 3, 1, [2.0, 4.0, 6.0]);
print_r($model->predict([4.0, 5.0], 2, 1));   // [8.0, 10.0]

// SQL Session
$sess = new Session();
$sess->loadTable('sales', [
    ['region' => 'North', 'amount' => 1000],
    ['region' => 'South', 'amount' => 2000],
]);
echo $sess->rowCount('sales') . "\n";   // 2
print_r($sess->query('SELECT SUM(amount) AS total FROM sales'));
// [['total' => 3000]]

$sess->registerBlock('blk', $blk);
print_r($sess->query('SELECT * FROM blk'));
```

---

## Environment variables

| Variable      | Purpose                                      | Default                  |
|---------------|----------------------------------------------|--------------------------|
| `KORE_LIB`    | Override path to shared library              | Auto-detected            |
| `KORE_API_URL`| Java / Node.js REST mode server URL          | `http://localhost:3000`  |

---

## Notes

- **Java**: uses HTTP REST; requires `cargo run --release -p kore-api` to be
  running.  All other languages call the shared library directly.
- **Node.js**: automatically selects native mode when `ffi-napi` is installed,
  otherwise falls back to REST mode.
- **Ruby**: uses Fiddle from the standard library — no gems required.
- **PHP**: requires `extension=ffi` in `php.ini`.  Available since PHP 7.4.
- **Thread safety**: each `KoreSession` is independent.  You may create
  multiple sessions in different threads.