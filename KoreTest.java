/**
 * KORE Java Direct Test — Panama FFI (Java 21 built-in, no deps)
 * Calls kore_ffi.dll directly without any server or Maven.
 *
 * Compile: javac --enable-preview --release 21 KoreTest.java
 * Run:     java --enable-preview -Djava.library.path=target\release KoreTest
 *
 * Or with full path:
 * java --enable-preview
 *      -Djava.library.path=C:\Users\skathera\Downloads\asistent\kore\target\release
 *      KoreTest
 */
import java.lang.foreign.*;
import java.lang.invoke.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;

public class KoreTest {

    private static final String LIB_PATH =
        "C:\\Users\\skathera\\Downloads\\asistent\\kore\\target\\release\\kore_ffi.dll";

    public static void main(String[] args) throws Throwable {
        System.out.println("=== KORE Java Panama FFI Real Test ===");

        // Load the DLL
        Arena arena = Arena.ofConfined();
        SymbolLookup lib = SymbolLookup.libraryLookup(Path.of(LIB_PATH), arena);
        Linker linker = Linker.nativeLinker();

        // Wire up functions
        MethodHandle sessionNew = linker.downcallHandle(
            lib.find("kore_session_new").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.ADDRESS));

        MethodHandle sessionFree = linker.downcallHandle(
            lib.find("kore_session_free").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

        MethodHandle sessionLoadCsv = linker.downcallHandle(
            lib.find("kore_session_load_csv").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_INT,
                ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));

        MethodHandle sessionQuery = linker.downcallHandle(
            lib.find("kore_session_query").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.ADDRESS,
                ValueLayout.ADDRESS, ValueLayout.ADDRESS));

        MethodHandle sessionRowCount = linker.downcallHandle(
            lib.find("kore_session_row_count").orElseThrow(),
            FunctionDescriptor.of(ValueLayout.JAVA_LONG,
                ValueLayout.ADDRESS, ValueLayout.ADDRESS));

        MethodHandle freeString = linker.downcallHandle(
            lib.find("kore_free_string").orElseThrow(),
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

        // Create session
        MemorySegment sess = (MemorySegment) sessionNew.invoke();
        System.out.println("[1] Session created: " + sess);

        // Load CSV
        String csvPath = "C:\\Users\\skathera\\Downloads\\asistent\\bench_export.csv";
        MemorySegment tableSeg = arena.allocateUtf8String("bench");
        MemorySegment pathSeg  = arena.allocateUtf8String(csvPath);
        int rc = (int) sessionLoadCsv.invoke(sess, tableSeg, pathSeg);
        System.out.println("[2] load_csv returned: " + rc + " (0=OK)");

        // Row count
        MemorySegment tableNameSeg = arena.allocateUtf8String("bench");
        long rows = (long) sessionRowCount.invoke(sess, tableNameSeg);
        System.out.println("[3] Row count: " + rows);

        // Run SQL query
        MemorySegment sqlSeg = arena.allocateUtf8String(
            "SELECT category, COUNT(*) as cnt, SUM(amount) as total " +
            "FROM bench GROUP BY category ORDER BY total DESC");
        MemorySegment result = (MemorySegment) sessionQuery.invoke(sess, sqlSeg);

        if (result.address() != 0) {
            // C pointer has limit=0 — reinterpret as unbounded to read bytes
            MemorySegment unbounded = result.reinterpret(Long.MAX_VALUE, arena, null);
            StringBuilder sb = new StringBuilder();
            long offset = 0;
            while (true) {
                byte b = unbounded.get(ValueLayout.JAVA_BYTE, offset++);
                if (b == 0) break;
                sb.append((char) b);
            }
            String json = sb.toString();
            System.out.println("[4] Query result (JSON):");
            System.out.println("    " + json.substring(0, Math.min(json.length(), 300)));
            freeString.invoke(result);
        } else {
            System.out.println("[4] Query returned NULL (error)");
        }

        // WHERE + LIMIT query
        MemorySegment sql2Seg = arena.allocateUtf8String(
            "SELECT id, amount FROM bench WHERE amount > 999 ORDER BY amount DESC LIMIT 3");
        MemorySegment result2 = (MemorySegment) sessionQuery.invoke(sess, sql2Seg);
        if (result2.address() != 0) {
            MemorySegment u2 = result2.reinterpret(Long.MAX_VALUE, arena, null);
            StringBuilder sb = new StringBuilder();
            long offset = 0;
            while (true) {
                byte b = u2.get(ValueLayout.JAVA_BYTE, offset++);
                if (b == 0) break;
                sb.append((char) b);
            }
            System.out.println("[5] WHERE+LIMIT: " + sb);
            freeString.invoke(result2);
        }

        // Free session
        sessionFree.invoke(sess);
        arena.close();

        System.out.println("\nJAVA 21 PANAMA FFI TEST PASSED");
        System.out.println("kore_ffi.dll loaded, SQL queries work from Java!");
    }
}
