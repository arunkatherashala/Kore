<?php
/**
 * KORE FileFormat v1.6.0 — PHP Genuine Test
 * Run: php test_v160_php.php
 * Requires: PHP 8.1+ with ext-ffi enabled
 */

define('VERSION',   '1.6.0');
define('REPO_ROOT', __DIR__);
define('DLL_PATH',  __DIR__ . '/target/release/kore_ffi.dll');

$passed = 0; $failed = 0;
$ts = gmdate('Y-m-d\TH:i:s\Z');

function check(string $label, bool $ok, string $note = ''): void {
    global $passed, $failed;
    $s = $ok ? ' PASS ' : ' FAIL ';
    echo "  [$s] $label" . ($note ? " — $note" : '') . PHP_EOL;
    $ok ? $passed++ : $failed++;
}

echo "======================================================================\n";
echo "  KORE FileFormat v" . VERSION . " — PHP Test\n";
echo "  PHP " . PHP_VERSION . " | Run: $ts\n";
echo "======================================================================\n";

// ── Test 1: Version ────────────────────────────────────────────────────────
echo "\n  [1] Version\n";
check('VERSION = 1.6.0',   VERSION === '1.6.0', VERSION);
check('PHP >= 8.0',        PHP_MAJOR_VERSION >= 8, PHP_VERSION);
check('ext-ffi enabled',   extension_loaded('ffi'), extension_loaded('ffi') ? 'yes' : 'no');
check('DLL exists',        file_exists(DLL_PATH), file_exists(DLL_PATH) ? round(filesize(DLL_PATH)/1024/1024, 1).'MB' : 'missing');

// ── Load FFI ──────────────────────────────────────────────────────────────
echo "\n  [2] Load kore_ffi.dll via PHP FFI\n";
try {
    $ffi = FFI::cdef(<<<EOH
        typedef void KoreBlock;
        uint32_t kore_crc32(const uint8_t* data, size_t len);
        KoreBlock* kore_block_new();
        void       kore_block_free(KoreBlock* block);
        int        kore_block_add_f64(KoreBlock* block, const char* name, const double* data, size_t len);
        int        kore_block_add_i64(KoreBlock* block, const char* name, const int64_t* data, size_t len);
        int        kore_write_file(const char* path, KoreBlock* block);
        KoreBlock* kore_read_file(const char* path);
        uint64_t   kore_block_num_rows(const KoreBlock* block);
        uint32_t   kore_block_num_cols(const KoreBlock* block);
        char*      kore_block_col_name(const KoreBlock* block, size_t idx);
        int64_t    kore_block_get_f64(const KoreBlock* block, const char* col, double* out, uint64_t maxlen);
        void       kore_free_string(char* s);
EOH, DLL_PATH);
    check('FFI::cdef() loaded DLL', true, DLL_PATH);
} catch (Throwable $e) {
    check('FFI::cdef() loaded DLL', false, $e->getMessage());
    exit(1);
}

// ── Test 3: CRC32 ─────────────────────────────────────────────────────────
echo "\n  [3] CRC32 — matches all other languages\n";
$str  = 'hello kore v1.6.0';
$buf  = $ffi->new("uint8_t[" . strlen($str) . "]");
FFI::memcpy($buf, $str, strlen($str));
$crc  = $ffi->kore_crc32($buf, strlen($str));
const EXPECTED_CRC = 0x5946aaf8;
check('crc32 non-zero',                   $crc !== 0,            sprintf('0x%08x', $crc));
check('crc32 = 0x5946aaf8 (all langs)',   $crc === EXPECTED_CRC, sprintf('0x%08x == 0x%08x', $crc, EXPECTED_CRC));

// ── Test 4: Write real data ────────────────────────────────────────────────
echo "\n  [4] Write real order data (10 rows, timestamped)\n";
$nowMs    = (int)(microtime(true) * 1000);
$prices   = [10.5, 20.0, 30.75, 15.0, 45.99, 8.25, 99.0, 55.5, 12.0, 33.33];
$orderIds = [1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009, 1010];
$tsMs     = array_map(fn($i) => $nowMs + $i * 60000, range(0, 9));

$handle = $ffi->kore_block_new();
check('kore_block_new()', !FFI::isNull($handle));

// Add price column (F64)
$priceArr = $ffi->new('double[10]');
foreach ($prices as $i => $v) $priceArr[$i] = $v;
$rc = $ffi->kore_block_add_f64($handle, 'price', $priceArr, 10);
check('add_f64 price column', $rc === 0, "rc=$rc");

// Add order_id column (I64)
$idArr = $ffi->new('int64_t[10]');
foreach ($orderIds as $i => $v) $idArr[$i] = $v;
$rc2 = $ffi->kore_block_add_i64($handle, 'order_id', $idArr, 10);
check('add_i64 order_id column', $rc2 === 0, "rc=$rc2");

// Add timestamp column (I64)
$tsArr = $ffi->new('int64_t[10]');
foreach ($tsMs as $i => $v) $tsArr[$i] = $v;
$ffi->kore_block_add_i64($handle, 'timestamp_ms', $tsArr, 10);

$outFile = REPO_ROOT . '/test_v160_php.kore';
$wrc = $ffi->kore_write_file($outFile, $handle);
$ffi->kore_block_free($handle);
check('write_file rc=0', $wrc === 0, "rc=$wrc");
check('.kore file created', file_exists($outFile), file_exists($outFile) ? filesize($outFile).' bytes' : 'missing');

// ── Test 5: Read back ─────────────────────────────────────────────────────
echo "\n  [5] Read back + verify\n";
$rHandle = $ffi->kore_read_file($outFile);
check('kore_read_file() ok', !FFI::isNull($rHandle));

if (!FFI::isNull($rHandle)) {
    $nrows = $ffi->kore_block_num_rows($rHandle);
    $ncols = $ffi->kore_block_num_cols($rHandle);
    check('10 rows',     $nrows === 10, "$nrows");
    check('3 columns',   $ncols === 3,  "$ncols");

    // Read price values back
    $outBuf = $ffi->new('double[10]');
    $n = $ffi->kore_block_get_f64($rHandle, 'price', $outBuf, 10);
    check('price readable',   $n > 0,                           "$n values");
    check('price[0] = 10.5',  abs($outBuf[0] - 10.5) < 0.001,  sprintf('%.2f', $outBuf[0]));
    check('price[9] = 33.33', abs($outBuf[9] - 33.33) < 0.001, sprintf('%.2f', $outBuf[9]));
    $ffi->kore_block_free($rHandle);
}
@unlink($outFile);

// ── Test 6: Cross-language binary ─────────────────────────────────────────
echo "\n  [6] Cross-language: Python-written .kore\n";
$pyFile = REPO_ROOT . '/test_v160_orders.kore';
check('Python .kore exists', file_exists($pyFile), file_exists($pyFile) ? filesize($pyFile).' bytes' : 'missing');
if (file_exists($pyFile)) {
    $raw   = file_get_contents($pyFile);
    $magic = substr($raw, 0, 4);
    check('Magic = KORE',    $magic === 'KORE',  "got '$magic'");
    $ver   = unpack('v', substr($raw, 4, 2))[1];
    check('Format version >= 1', $ver >= 1,      "v$ver");
    $ncols = unpack('V', substr($raw, 6, 4))[1];
    check('4 columns',       $ncols === 4,        "$ncols");
    $nrows = unpack('P', substr($raw, 10, 8))[1];  // P = 64-bit LE
    check('10 rows',         $nrows === 10,        "$nrows");
}

// ── Summary ────────────────────────────────────────────────────────────────
$total = $passed + $failed;
echo "\n";
echo "======================================================================\n";
echo "  PHP " . PHP_VERSION . " | KORE v" . VERSION . " | $ts\n";
echo "  TOTAL: $passed/$total passed | $failed failed\n";
echo "======================================================================\n";
exit($failed > 0 ? 1 : 0);
