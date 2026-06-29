<?php
/**
 * kore.php — PHP 8.1+ FFI bindings for the KORE engine.
 *
 * Requirements: PHP 8.1+, ext-ffi enabled in php.ini
 * Build first:  cargo build --release -p kore-ffi
 *
 * Usage:
 *   require_once 'kore.php';
 *   $k = new Kore\Engine();
 *   $block = $k->newBlock();
 *   $block->addF64('score', [1.0, 2.0, 3.0]);
 *   $model = $k->newModel(Kore\ModelType::LINEAR_REGRESSOR);
 *   $model->fit($xFlat, $nRows, $nCols, $y);
 *   $preds = $model->predict($xFlat, $nRows, $nCols);
 */

declare(strict_types=1);
namespace Kore;

// ── FFI setup ─────────────────────────────────────────────────────────────────

function _load_lib(): \FFI
{
    $lib = getenv('KORE_LIB') ?: _find_lib();
    return \FFI::cdef(<<<C
        const char* kore_last_error(void);
        void*       kore_block_new(void);
        void        kore_block_free(void* ptr);
        uint64_t    kore_block_num_rows(const void* ptr);
        uint32_t    kore_block_num_cols(const void* ptr);
        int         kore_block_add_f64(void* ptr, const char* name, const double* data, uint64_t len);
        int         kore_block_add_i64(void* ptr, const char* name, const int64_t* data, uint64_t len);
        int64_t     kore_block_get_f64(const void* ptr, const char* col, double* out, uint64_t maxlen);
        void*       kore_hash_join(const void* left, const void* right,
                                   const char* lk, const char* rk, int jtype);
        void*       kore_model_new(int type, int p1, int p2);
        void        kore_model_free(void* ptr);
        int         kore_model_fit(void* ptr, const double* x, uint64_t rows, uint64_t cols, const double* y);
        int         kore_model_predict(const void* ptr, const double* x,
                                       uint64_t rows, uint64_t cols, double* out);
    C, $lib);
}

function _find_lib(): string
{
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
        "libkore_ffi not found.\nBuild: cargo build --release -p kore-ffi\n" .
        "Then set KORE_LIB=/path/to/lib"
    );
}

$_KORE_FFI = null;
function ffi(): \FFI
{
    global $_KORE_FFI;
    return $_KORE_FFI ??= _load_lib();
}

function _check(int $rc): void
{
    if ($rc !== 0) {
        $msg = ffi()->kore_last_error();
        throw new \RuntimeException("KORE: " . ($msg !== null ? $msg : "error $rc"));
    }
}

// ── ModelType ─────────────────────────────────────────────────────────────────

class ModelType
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

// ── Block ─────────────────────────────────────────────────────────────────────

class Block
{
    private $ptr;

    public function __construct($ptr = null)
    {
        $this->ptr = $ptr ?? ffi()->kore_block_new();
        if ($this->ptr === null)
            throw new \RuntimeException('kore_block_new returned null');
    }

    public function __destruct() { if ($this->ptr) ffi()->kore_block_free($this->ptr); }

    public function numRows(): int { return (int) ffi()->kore_block_num_rows($this->ptr); }
    public function numCols(): int { return (int) ffi()->kore_block_num_cols($this->ptr); }

    public function addF64(string $name, array $data): static
    {
        $n   = count($data);
        $buf = \FFI::new("double[$n]");
        foreach ($data as $i => $v) $buf[$i] = (float)$v;
        _check(ffi()->kore_block_add_f64($this->ptr, $name, $buf, $n));
        return $this;
    }

    public function addI64(string $name, array $data): static
    {
        $n   = count($data);
        $buf = \FFI::new("int64_t[$n]");
        foreach ($data as $i => $v) $buf[$i] = (int)$v;
        _check(ffi()->kore_block_add_i64($this->ptr, $name, $buf, $n));
        return $this;
    }

    public function getF64(string $col): array
    {
        $n   = $this->numRows();
        $buf = \FFI::new("double[$n]");
        $rc  = ffi()->kore_block_get_f64($this->ptr, $col, $buf, $n);
        if ($rc < 0) throw new \RuntimeException('kore_block_get_f64: ' . ffi()->kore_last_error());
        $out = [];
        for ($i = 0; $i < $rc; $i++) $out[] = $buf[$i];
        return $out;
    }

    public function hashJoin(Block $right, string $lk, string $rk, int $how = 0): static
    {
        $ptr = ffi()->kore_hash_join($this->ptr, $right->ptr, $lk, $rk, $how);
        if ($ptr === null) throw new \RuntimeException('hash_join: ' . ffi()->kore_last_error());
        return new static($ptr);
    }

    public function __toString(): string { return "KoreBlock(rows={$this->numRows()}, cols={$this->numCols()})"; }
}

// ── Model ─────────────────────────────────────────────────────────────────────

class Model
{
    private $ptr;

    public function __construct(int $type, int $p1 = 100, int $p2 = 3)
    {
        $this->ptr = ffi()->kore_model_new($type, $p1, $p2);
        if ($this->ptr === null)
            throw new \RuntimeException('kore_model_new failed: ' . ffi()->kore_last_error());
    }

    public function __destruct() { if ($this->ptr) ffi()->kore_model_free($this->ptr); }

    /** @param float[] $xFlat row-major, length = $nRows * $nCols */
    public function fit(array $xFlat, int $nRows, int $nCols, array $y): static
    {
        $nx = count($xFlat);
        $xb = \FFI::new("double[$nx]");
        foreach ($xFlat as $i => $v) $xb[$i] = (float)$v;
        $yb = \FFI::new("double[$nRows]");
        foreach ($y as $i => $v) $yb[$i] = (float)$v;
        _check(ffi()->kore_model_fit($this->ptr, $xb, $nRows, $nCols, $yb));
        return $this;
    }

    /** @return float[] */
    public function predict(array $xFlat, int $nRows, int $nCols): array
    {
        $nx = count($xFlat);
        $xb = \FFI::new("double[$nx]");
        foreach ($xFlat as $i => $v) $xb[$i] = (float)$v;
        $ob = \FFI::new("double[$nRows]");
        _check(ffi()->kore_model_predict($this->ptr, $xb, $nRows, $nCols, $ob));
        $out = [];
        for ($i = 0; $i < $nRows; $i++) $out[] = $ob[$i];
        return $out;
    }
}

// ── Engine (factory) ──────────────────────────────────────────────────────────

class Engine
{
    public function newBlock(): Block { return new Block(); }
    public function newModel(int $type, int $p1 = 100, int $p2 = 3): Model { return new Model($type, $p1, $p2); }
}
