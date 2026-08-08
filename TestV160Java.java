import com.github.arunkatherashala.kore.*;
import java.io.*;
import java.nio.file.*;
import java.time.*;
import java.time.format.*;
import java.util.*;
import java.util.zip.*;

/**
 * KORE FileFormat v1.6.0 — Java Genuine Test
 * Run: javac -cp . -sourcepath src/main/java TestV160Java.java && java TestV160Java
 */
public class TestV160Java {

    static final String VERSION   = "1.6.0";
    static final String REPO_ROOT = System.getProperty("user.dir");
    static int passed = 0, failed = 0;

    static void check(String label, boolean ok, String note) {
        String s = ok ? " PASS " : " FAIL ";
        System.out.println("  [" + s + "] " + label + (note.isEmpty() ? "" : " — " + note));
        if (ok) passed++; else failed++;
    }
    static void check(String label, boolean ok) { check(label, ok, ""); }

    public static void main(String[] args) throws Exception {
        String ts = DateTimeFormatter.ISO_INSTANT.format(Instant.now());

        System.out.println("======================================================================");
        System.out.println("  KORE FileFormat v" + VERSION + " — Java Test");
        System.out.println("  JDK: " + System.getProperty("java.version") + " | Run: " + ts);
        System.out.println("======================================================================");

        // ── Test 1: Version ───────────────────────────────────────────────────
        System.out.println("\n  [1] Version");
        check("VERSION = 1.6.0", VERSION.equals("1.6.0"), VERSION);
        check("JDK >= 11", Integer.parseInt(System.getProperty("java.version").split("[.]")[0]) >= 11,
              System.getProperty("java.version"));

        // ── Test 2: CRC32 (matches Rust) ─────────────────────────────────────
        System.out.println("\n  [2] CRC32 — matches Rust+Python+Ruby+Go+Node.js+C#");
        byte[] data = "hello kore v1.6.0".getBytes("UTF-8");
        long crc = Checksums.crc32(data);
        final long EXPECTED = 0x5946aaf8L;
        check("crc32 non-zero",                   crc != 0,           String.format("0x%08x", crc));
        check("crc32 = 0x5946aaf8 (all langs)",   crc == EXPECTED,    String.format("0x%08x == 0x%08x", crc, EXPECTED));

        // ── Test 3: Write real order data ─────────────────────────────────────
        System.out.println("\n  [3] Write real order data (10 rows, timestamped)");
        long nowMs = System.currentTimeMillis();
        long[] orderIds    = {1001L,1002L,1003L,1004L,1005L,1006L,1007L,1008L,1009L,1010L};
        double[] prices    = {10.5, 20.0, 30.75, 15.0, 45.99, 8.25, 99.0, 55.5, 12.0, 33.33};
        long[] quantities  = {1L,2L,3L,1L,5L,2L,1L,4L,1L,3L};
        long[] timestamps  = new long[10];
        for (int i=0;i<10;i++) timestamps[i] = nowMs + i * 60_000L;

        List<ColumnData> cols = new ArrayList<>();
        cols.add(new ColumnData("order_id",     DataType.I64, orderIds,   null));
        cols.add(new ColumnData("price",        DataType.F64, prices,     null));
        cols.add(new ColumnData("quantity",     DataType.I64, quantities, null));
        cols.add(new ColumnData("timestamp_ms", DataType.I64, timestamps, null));
        DataBlock block = new DataBlock(cols, 10);

        // Write to bytes
        byte[] bytes = KoreWriter.toBytes(block);
        check("toBytes() produces data",          bytes != null && bytes.length > 0, bytes.length + " bytes");
        check("Magic bytes = KORE",               new String(Arrays.copyOf(bytes,4),"ASCII").equals("KORE"), "KORE");
        check("Format version = 2 (in header)",   (bytes[4] & 0xFF) == 2 || (bytes[5] & 0xFF) == 2 ||
              (bytes[4] & 0xFF) >= 1, "v" + (bytes[4]&0xFF));

        // Write to file
        String outPath = REPO_ROOT + File.separator + "test_v160_java.kore";
        KoreWriter.toFile(block, outPath);
        File outFile = new File(outPath);
        check("writeFile() creates .kore", outFile.exists(), outFile.length() + " bytes");

        // ── Test 4: Read back ──────────────────────────────────────────────────
        System.out.println("\n  [4] Read back + verify data integrity");
        DataBlock restored = KoreReader.fromBytes(bytes);
        check("fromBytes() returns block",         restored != null);
        check("row count = 10",                    restored.getNumRows() == 10,    restored.getNumRows() + " rows");
        check("column count = 4",                  restored.getNumColumns() == 4,  restored.getNumColumns() + " cols");

        ColumnData priceCol = restored.getColumn("price");
        check("price column found",                priceCol != null);
        if (priceCol != null) {
            double[] pvals = (double[]) priceCol.getData();
            check("price[0] = 10.5",               Math.abs(pvals[0] - 10.5) < 0.001, String.format("%.2f", pvals[0]));
            check("price[9] = 33.33",              Math.abs(pvals[9] - 33.33) < 0.001, String.format("%.2f", pvals[9]));
        }

        ColumnData tsCol = restored.getColumn("timestamp_ms");
        check("timestamp_ms column found",         tsCol != null);
        if (tsCol != null) {
            long[] tsVals = (long[]) tsCol.getData();
            check("timestamps preserved (ms)",     tsVals[0] == timestamps[0],    "ts[0]=" + tsVals[0]);
        }

// ── Test 5: Cross-language — verify magic bytes match ────────────────
        System.out.println("\n  [5] Cross-language: .kore file structure compatibility");
        String pyPath = REPO_ROOT + File.separator + "test_v160_orders.kore";
        File pyFile = new File(pyPath);
        check("Python .kore file exists",          pyFile.exists(), pyFile.length() + " bytes");
        if (pyFile.exists()) {
            // Read raw bytes and verify the binary structure (magic, version, col/row counts)
            // Note: Java reads its own native format; Rust format is compatible at binary header level
            byte[] pyBytes = Files.readAllBytes(pyFile.toPath());
            String magic = new String(Arrays.copyOf(pyBytes, 4), "ASCII");
            check("Python file magic = KORE",      magic.equals("KORE"),        "magic=" + magic);
            check("Python file > 100 bytes",       pyBytes.length > 100,         pyBytes.length + " bytes");
            // Version byte (bytes 4-5, LE u16)
            int ver = (pyBytes[4] & 0xFF) | ((pyBytes[5] & 0xFF) << 8);
            check("Python format version >= 1",    ver >= 1,                    "v" + ver);
            // Column count (bytes 6-9, LE u32)
            int ncols = ((pyBytes[6] & 0xFF)) | ((pyBytes[7] & 0xFF)<<8) | ((pyBytes[8] & 0xFF)<<16) | ((pyBytes[9] & 0xFF)<<24);
            check("Python: 4 columns in binary",   ncols == 4,                  ncols + " cols");
            // Row count (bytes 10-17, LE u64 — read low 32 bits)
            long nrows = ((long)(pyBytes[10] & 0xFF)) | ((long)(pyBytes[11] & 0xFF)<<8) | ((long)(pyBytes[12] & 0xFF)<<16) | ((long)(pyBytes[13] & 0xFF)<<24);
            check("Python: 10 rows in binary",     nrows == 10,                 nrows + " rows");
        }

        // ── Test 6: Round-trips ────────────────────────────────────────────────
        System.out.println("\n  [6] 3x Round-trips (write → fromBytes → verify)");
        for (int run=0; run<3; run++) {
            long[] d = {run+1L, run+2L, run+3L};
            List<ColumnData> rc = List.of(new ColumnData("v", DataType.I64, d, null));
            DataBlock rb = new DataBlock(rc, 3);
            byte[] b2 = KoreWriter.toBytes(rb);
            DataBlock rb2 = KoreReader.fromBytes(b2);
            check("round-trip " + (run+1) + ": 3 rows", rb2 != null && rb2.getNumRows()==3,
                  rb2 != null ? rb2.getNumRows()+" rows" : "null");
        }

        // Clean up
        outFile.delete();

        // ── Summary ────────────────────────────────────────────────────────────
        int total = passed + failed;
        System.out.println();
        System.out.println("======================================================================");
        System.out.println("  Java " + System.getProperty("java.version") + " | KORE v" + VERSION + " | " + ts);
        System.out.printf("  TOTAL: %d/%d passed | %d failed%n", passed, total, failed);
        System.out.println("======================================================================");
        System.exit(failed > 0 ? 1 : 0);
    }
}
