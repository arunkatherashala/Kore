<?php
/**
 * kore.php -- PHP bindings for the KORE engine using PHP FFI (PHP 7.4+).
 *
 * Covers:
 *   * DataBlock / ML API   (Kore\Block, Kore\Model, Kore\Engine)
 *   * SQL Session API      (Kore\Session / KoreSession convenience class)
 *
 * Requirements:
 *   PHP 7.4+ with FFI extension enabled (extension=ffi in php.ini).
 *   Build first: cargo build --release -p kore-ffi
 *
 * Usage:
 *   require_once 'kore.php';
 *   $sess = new Kore\Session();
 *   $sess->loadTable('sales', [
 *       ['region' => 'North', 'amount' => 1000],
 *       ['region' => 'South', 'amount' => 2000],
 *   ]);
 *   print_r($sess->query('SELECT region, SUM(amount) AS total FROM sales GROUP BY region'));
 */

declare(strict_types=1);

namespace Kore;

// =============================================================================
// FFI bootstrap
// =============================================================================

function _findLib(): string
{
    $env = getenv('KORE_LIB');
    if ($env !== false && $env !== '') return $env;

    $root = dirname(__DIR__, 3);
    $candidates = [
        "$root/target/release/kore_ffi.dll",
        "$root/target/release/libkore_ffi.so",
        "$root/target/release/libkore_ffi.dylib",
    ];
    foreach ($candidates as $p) {
        if (file_exists($p)) return $p;
    }
    throw new \RuntimeException(
        "KORE shared library not found.\n" .
        "Build with: cargo build --release -p kore-ffi\n" .
        "Then set KORE_LIB=/path/to/lib"
    );
}

function _loadFfi(): \FFI
{
    $lib = _findLib();
    return \FFI::cdef(<<<'C'
        /* Error */
        const char* kore_last_error(void);

        /* DataBlock */
        void*       kore_block_new(void);
        void        kore_block_free(void* ptr);
        uint64_t    kore_block_num_rows(const void* ptr);
        uint32_t    kore_block_num_cols(const void* ptr);
        int         kore_block_add_f64(void* ptr, const char* name,
                                       const double* data, uint64_t len);
        int         kore_block_add_i64(void* ptr, const char* name,
                                       const int64_t* data, uint64_t len);
        int64_t     kore_block_get_f64(const void* ptr, const char* col,
                                       double* out, uint64_t maxlen);
        void*       kore_hash_join(const void* left, const void* right,
                                   const char* lk, const char* rk, int jtype);

        /* ML Models */
        void*       kore_model_new(int type, int p1, int p2);
        void        kore_model_free(void* ptr);
        int         kore_model_fit(void* ptr, const double* x,
                                   uint64_t rows, uint64_t cols,
                                   const double* y);
        int         kore_model_predict(const void* ptr, const double* x,
                                       uint64_t rows, uint64_t cols,
                                       double* out);

        /* SQL Session */
        void*       kore_session_new(void);
        void        kore_session_free(void* sess);
        int         kore_session_load_csv(void* sess,
                                          const char* table_name,
                                          const char* path);
        int         kore_session_register_block(void* sess,
                                                const char* table_name,
                                                const void* block);
        char*       kore_session_query(void* sess, const char* sql);
        int64_t     kore_session_row_count(const void* sess,
                                           const char* table_name);
        void        kore_free_string(char* s);
    C, $lib);
}

/** Returns the singleton FFI instance (lazy-loaded). */
function ffi(): \FFI
{
    static $inst = null;
    return $inst ??= _loadFfi();
}

function _checkRc(int $rc): void
{
    if ($rc !== 0) {
        $msg = ffi()->kore_last_error();
        throw new \RuntimeException('KORE: ' . ($msg !== null ? $msg : "error $rc"));
    }
}

function _checkPtr(mixed $ptr): mixed
{
    if ($ptr === null || \FFI::isNull($ptr)) {
        $msg = ffi()->kore_last_error();
        throw new \RuntimeException('KORE: ' . ($msg !== null ? $msg : 'NULL pointer'));
    }
    return $ptr;
}

// =============================================================================
// ModelType
// =============================================================================

final class ModelType
{
    const RF_REGRESSOR     = 0;
    const RF_CLASSIFIER    = 1;
    const GBM_REGRESSOR    = 2;
    const LINEAR_REGRESSOR = 3;
    const LOGISTIC         = 4;
    const KNN_REGRESSOR    = 5;
    const KNN_CLASSIFIER   = 6;
    const SVM              = 7;
}

// =============================================================================
// Block
// =============================================================================

class Block
{
    /** @var mixed */
    private mixed $ptr;

    public function __construct(mixed $ptr = null)
    {
        $this->ptr = $ptr ?? _checkPtr(ffi()->kore_block_new());
    }

    public function __destruct()
    {
        if ($this->ptr !== null) {
            ffi()->kore_block_free($this->ptr);
            $this->ptr = null;
        }
    }

    public function numRows(): int { return (int) ffi()->kore_block_num_rows($this->ptr); }
    public function numCols(): int { return (int) ffi()->kore_block_num_cols($this->ptr); }

    /** @param float[] $data */
    public function addF64(string $name, array $data): static
    {
        $n   = count($data);
        $buf = \FFI::new("double[$n]");
        foreach ($data as $i => $v) $buf[$i] = (float) $v;
        _checkRc(ffi()->kore_block_add_f64($this->ptr, $name, $buf, $n));
        return $this;
    }

    /** @param int[] $data */
    public function addI64(string $name, array $data): static
    {
        $n   = count($data);
        $buf = \FFI::new("int64_t[$n]");
        foreach ($data as $i => $v) $buf[$i] = (int) $v;
        _checkRc(ffi()->kore_block_add_i64($this->ptr, $name, $buf, $n));
        return $this;
    }

    /** @return float[] */
    public function getF64(string $col): array
    {
        $n   = $this->numRows();
        $buf = \FFI::new("double[$n]");
        $rc  = ffi()->kore_block_get_f64($this->ptr, $col, $buf, $n);
        if ($rc < 0) throw new \RuntimeException('kore_block_get_f64: ' . ffi()->kore_last_error());
        $out = [];
        for ($i = 0; $i < $rc; $i++) $out[] = (float) $buf[$i];
        return $out;
    }

    /** @param int $how 0=INNER 1=LEFT 2=FULL */
    public function hashJoin(Block $right, string $lk, string $rk, int $how = 0): static
    {
        $ptr = ffi()->kore_hash_join($this->ptr, $right->ptr, $lk, $rk, $how);
        return new static(_checkPtr($ptr));
    }

    /** @internal for register_block() */
    public function rawPtr(): mixed { return $this->ptr; }

    public function __toString(): string
    {
        return sprintf('KoreBlock(rows=%d, cols=%d)', $this->numRows(), $this->numCols());
    }
}

// =============================================================================
// Model
// =============================================================================

class Model
{
    private mixed $ptr;

    public function __construct(int $type, int $p1 = 100, int $p2 = 3)
    {
        $this->ptr = _checkPtr(ffi()->kore_model_new($type, $p1, $p2));
    }

    public function __destruct()
    {
        if ($this->ptr !== null) {
            ffi()->kore_model_free($this->ptr);
            $this->ptr = null;
        }
    }

    /**
     * @param float[] $xFlat row-major, length = $nRows * $nCols
     * @param float[] $y
     */
    public function fit(array $xFlat, int $nRows, int $nCols, array $y): static
    {
        $nx = count($xFlat);
        $xb = \FFI::new("double[$nx]");
        foreach ($xFlat as $i => $v) $xb[$i] = (float) $v;
        $yb = \FFI::new("double[$nRows]");
        foreach ($y as $i => $v) $yb[$i] = (float) $v;
        _checkRc(ffi()->kore_model_fit($this->ptr, $xb, $nRows, $nCols, $yb));
        return $this;
    }

    /** @return float[] */
    public function predict(array $xFlat, int $nRows, int $nCols): array
    {
        $nx = count($xFlat);
        $xb = \FFI::new("double[$nx]");
        foreach ($xFlat as $i => $v) $xb[$i] = (float) $v;
        $ob = \FFI::new("double[$nRows]");
        _checkRc(ffi()->kore_model_predict($this->ptr, $xb, $nRows, $nCols, $ob));
        $out = [];
        for ($i = 0; $i < $nRows; $i++) $out[] = (float) $ob[$i];
        return $out;
    }
}

// =============================================================================
// Session
// =============================================================================

class Session
{
    private mixed $handle;

    public function __construct()
    {
        $this->handle = _checkPtr(ffi()->kore_session_new());
    }

    public function __destruct()
    {
        if ($this->handle !== null) {
            ffi()->kore_session_free($this->handle);
            $this->handle = null;
        }
    }

    // -------------------------------------------------------------------------
    // Data loading
    // -------------------------------------------------------------------------

    /** Register a CSV file on disk as a named table. */
    public function loadCsv(string $table, string $path): static
    {
        $abs = realpath($path) ?: $path;
        _checkRc(ffi()->kore_session_load_csv($this->handle, $table, $abs));
        return $this;
    }

    /**
     * Load an array of associative arrays as a named table.
     * Data is written to a temporary CSV file then imported.
     *
     * @param array[] $data
     */
    public function loadTable(string $table, array $data): static
    {
        if (empty($data)) throw new \InvalidArgumentException('data must not be empty');
        $cols  = array_keys($data[0]);
        $lines = [implode(',', array_map([$this, '_csvEscape'], $cols))];
        foreach ($data as $row) {
            $fields = [];
            foreach ($cols as $c) {
                $v = (string)($row[$c] ?? '');
                $fields[] = $this->_csvEscape($v);
            }
            $lines[] = implode(',', $fields);
        }
        $tmp = tempnam(sys_get_temp_dir(), 'kore_') . '.csv';
        file_put_contents($tmp, implode("\n", $lines));
        try {
            $this->loadCsv($table, $tmp);
        } finally {
            @unlink($tmp);
        }
        return $this;
    }

    private function _csvEscape(string $v): string
    {
        if (strpos($v, ',') !== false || strpos($v, '"') !== false || strpos($v, "\n") !== false) {
            return '"' . str_replace('"', '""', $v) . '"';
        }
        return $v;
    }

    /** Register a Block as a named SQL table (data is copied). */
    public function registerBlock(string $table, Block $block): static
    {
        _checkRc(ffi()->kore_session_register_block(
            $this->handle, $table, $block->rawPtr()
        ));
        return $this;
    }

    // -------------------------------------------------------------------------
    // Query
    // -------------------------------------------------------------------------

    /**
     * Execute a SQL query and return results as an array of associative arrays.
     *
     * @return array[]
     */
    public function query(string $sql): array
    {
        $rawPtr = ffi()->kore_session_query($this->handle, $sql);
        if ($rawPtr === null || \FFI::isNull($rawPtr)) {
            $msg = ffi()->kore_last_error();
            throw new \RuntimeException('KORE query error: ' . ($msg ?? 'NULL result'));
        }
        // Read the heap string and free it
        $jsonStr = \FFI::string($rawPtr);
        ffi()->kore_free_string($rawPtr);
        $result = json_decode($jsonStr, true, 512, JSON_THROW_ON_ERROR);
        return is_array($result) ? $result : [];
    }

    // -------------------------------------------------------------------------
    // Metadata
    // -------------------------------------------------------------------------

    public function rowCount(string $table): int
    {
        $n = (int) ffi()->kore_session_row_count($this->handle, $table);
        if ($n < 0) throw new \RuntimeException("Table '$table' not found");
        return $n;
    }

    public function __toString(): string
    {
        return 'KoreSession';
    }
}

// =============================================================================
// Engine (factory -- backwards compatible)
// =============================================================================

class Engine
{
    public function newBlock(): Block { return new Block(); }
    public function newModel(int $type, int $p1 = 100, int $p2 = 3): Model { return new Model($type, $p1, $p2); }
    public function newSession(): Session { return new Session(); }
}

// =============================================================================
// Top-level convenience alias
// =============================================================================

// Provide \KoreSession as a convenience outside of namespace
class_alias(Session::class, 'KoreSession');

// =============================================================================
// Demo (run directly: php kore.php)
// =============================================================================
if (basename(__FILE__) === basename($_SERVER['PHP_SELF'] ?? '')) {
    echo "=== KORE PHP bindings smoke test ===\n\n";

    // DataBlock
    echo "1. DataBlock API\n";
    $blk = new Block();
    $blk->addF64('x', [1.0, 2.0, 3.0, 4.0]);
    $blk->addI64('id', [10, 20, 30, 40]);
    echo "   $blk\n";
    echo '   x column: ' . implode(', ', $blk->getF64('x')) . "\n";

    // ML Model
    echo "\n2. ML Model (LinearRegressor)\n";
    $model = new Model(ModelType::LINEAR_REGRESSOR);
    $xFlat = [1.0, 2.0, 3.0, 4.0, 5.0];
    $y     = [2.0, 4.0, 6.0, 8.0, 10.0];
    $model->fit($xFlat, 5, 1, $y);
    $preds = $model->predict([6.0, 7.0], 2, 1);
    echo '   Predictions for x=6,7: ' . implode(', ', $preds) . "\n";

    // SQL Session
    echo "\n3. SQL Session API\n";
    $sess = new Session();
    $sess->loadTable('sales', [
        ['region' => 'North', 'amount' => 1000],
        ['region' => 'South', 'amount' => 2000],
        ['region' => 'North', 'amount' => 500],
    ]);
    echo '   row_count: ' . $sess->rowCount('sales') . "\n";
    $agg = $sess->query('SELECT region, SUM(amount) AS total FROM sales GROUP BY region');
    echo "   Aggregation:\n";
    foreach ($agg as $row) echo '     ' . print_r($row, true);

    echo "\n4. register_block -> SQL\n";
    $sess2 = new Session();
    $sess2->registerBlock('blk', $blk);
    $result = $sess2->query('SELECT SUM(x) AS s FROM blk');
    echo '   SUM(x): ' . print_r($result, true);

    echo "\nAll tests passed.\n";
}