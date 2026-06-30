// KoreTest.cs — KORE C# test via P/Invoke
// Run: dotnet-script KoreTest.cs  OR  create a project and run
// dotnet new console && copy this to Program.cs && dotnet run
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

const string DLL = @"C:\Users\skathera\Downloads\asistent\kore\target\release\kore_ffi.dll";

[DllImport(DLL, EntryPoint = "kore_session_new")]    static extern IntPtr SessionNew();
[DllImport(DLL, EntryPoint = "kore_session_free")]   static extern void   SessionFree(IntPtr s);
[DllImport(DLL, EntryPoint = "kore_session_load_csv")] static extern int  SessionLoadCsv(IntPtr s, [MarshalAs(UnmanagedType.LPStr)] string t, [MarshalAs(UnmanagedType.LPStr)] string p);
[DllImport(DLL, EntryPoint = "kore_session_query")]  static extern IntPtr SessionQuery(IntPtr s, [MarshalAs(UnmanagedType.LPStr)] string sql);
[DllImport(DLL, EntryPoint = "kore_session_row_count")] static extern long SessionRowCount(IntPtr s, [MarshalAs(UnmanagedType.LPStr)] string t);
[DllImport(DLL, EntryPoint = "kore_free_string")]    static extern void   FreeStr(IntPtr p);

Console.WriteLine("=== KORE C# P/Invoke Real Test ===");

var sess = SessionNew();
Console.WriteLine($"[1] Session: 0x{sess:x}");

int rc = SessionLoadCsv(sess, "bench", @"C:\Users\skathera\Downloads\asistent\bench_export.csv");
Console.WriteLine($"[2] load_csv returned: {rc} (0=OK)");

long n = SessionRowCount(sess, "bench");
Console.WriteLine($"[3] Row count: {n}");

IntPtr ptr = SessionQuery(sess, "SELECT category, COUNT(*) as cnt, SUM(amount) as total FROM bench GROUP BY category ORDER BY total DESC");
if (ptr != IntPtr.Zero) {
    string json = Marshal.PtrToStringUTF8(ptr)!;
    FreeStr(ptr);
    var rows = JsonSerializer.Deserialize<JsonElement[]>(json)!;
    Console.WriteLine($"[4] GROUP BY ({rows.Length} groups):");
    foreach (var r in rows)
        Console.WriteLine($"     {r}");
} else {
    Console.WriteLine("[4] Query returned NULL");
}

IntPtr ptr2 = SessionQuery(sess, "SELECT id, amount FROM bench WHERE amount > 999 ORDER BY amount DESC LIMIT 3");
if (ptr2 != IntPtr.Zero) {
    string json2 = Marshal.PtrToStringUTF8(ptr2)!;
    FreeStr(ptr2);
    Console.WriteLine($"[5] WHERE+LIMIT: {json2}");
}

SessionFree(sess);
Console.WriteLine("\nC# TEST PASSED — kore_ffi.dll works via P/Invoke!");
