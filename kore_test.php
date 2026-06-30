<?php
echo "=== KORE PHP FFI Real Test ===\n";

$dll = "C:\\Users\\skathera\\Downloads\\asistent\\kore\\target\\release\\kore_ffi.dll";
$csv = "C:\\Users\\skathera\\Downloads\\asistent\\bench_export.csv";

$ffi = FFI::cdef(
    "void* kore_session_new();
     void kore_session_free(void* session);
     int kore_session_load_csv(void* session, const char* path, const char* name);
     char* kore_session_query(void* session, const char* sql);
     long long kore_session_row_count(void* session);
     void kore_free_string(char* s);",
    $dll
);

$sess = $ffi->kore_session_new();
echo "[1] Session created\n";

$ret = $ffi->kore_session_load_csv($sess, "bench", $csv);
echo "[2] load_csv returned: $ret (0=OK)\n";

$count = $ffi->kore_session_row_count($sess);
echo "[3] Row count: $count\n";

$p = $ffi->kore_session_query($sess,
    "SELECT category, COUNT(*) as cnt, SUM(amount) as total FROM bench GROUP BY category");
$groups = FFI::string($p);
$ffi->kore_free_string($p);
$rows = json_decode($groups, true);
echo "[4] GROUP BY (" . count($rows) . " groups):\n";
foreach ($rows as $row) {
    echo "     " . json_encode($row) . "\n";
}

$p2 = $ffi->kore_session_query($sess,
    "SELECT id, amount FROM bench WHERE amount >= 999.99 ORDER BY amount DESC LIMIT 3");
$where_res = FFI::string($p2);
$ffi->kore_free_string($p2);
echo "[5] WHERE+LIMIT: $where_res\n";

$ffi->kore_session_free($sess);
echo "\nPHP TEST PASSED — kore_ffi.dll works via PHP FFI!\n";
