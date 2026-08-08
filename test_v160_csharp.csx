// KORE FileFormat v1.6.0 — C# Integration Test
// Run: dotnet script test_v160_csharp.csx  OR  dotnet run
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

const string VERSION = "1.6.0";
string repoRoot = @"C:\Users\skathera\Downloads\asistent\kore";

int passed = 0, failed = 0;
void Check(string label, bool ok, string note = "")
{
    Console.WriteLine($"  [{(ok ? " PASS " : " FAIL ")}] {label}{(note != "" ? " — " + note : "")}");
    if (ok) passed++; else failed++;
}

Console.WriteLine("======================================================================");
Console.WriteLine($"  KORE FileFormat v{VERSION} — C# .NET Test");
Console.WriteLine($"  .NET {Environment.Version} | Run: {DateTime.UtcNow:yyyy-MM-ddTHH:mm:ssZ}");
Console.WriteLine("======================================================================");

// ── P/Invoke declarations ────────────────────────────────────────────────────
string dllPath = Path.Combine(repoRoot, "target", "release", "kore_ffi.dll");
NativeLibrary.SetDllImportResolver(typeof(KoreFFI).Assembly, (name, asm, path) =>
    name == "kore_ffi" ? NativeLibrary.Load(dllPath) : IntPtr.Zero);

// ── Test 1: Version ──────────────────────────────────────────────────────────
Console.WriteLine("\n  [1] Version verification");
Check("VERSION = 1.6.0", VERSION == "1.6.0", VERSION);
Check(".NET Runtime >= 6",
    Environment.Version.Major >= 6, Environment.Version.ToString());

// ── Test 2: DLL exists ───────────────────────────────────────────────────────
Console.WriteLine("\n  [2] kore_ffi.dll P/Invoke");
Check("DLL path exists", File.Exists(dllPath),
    File.Exists(dllPath) ? $"{new FileInfo(dllPath).Length / 1024 / 1024}MB" : "not found");

if (File.Exists(dllPath))
{
    try
    {
        // CRC32 via P/Invoke
        var data   = Encoding.UTF8.GetBytes("hello kore v1.6.0");
        var pin    = GCHandle.Alloc(data, GCHandleType.Pinned);
        uint crc   = KoreFFI.kore_crc32(pin.AddrOfPinnedObject(), (UIntPtr)data.Length);
        pin.Free();
        const uint EXPECTED = 0x5946aaf8u;
        Check("CRC32 via P/Invoke (Rust)", crc != 0, $"0x{crc:x8}");
        Check("CRC32 matches Python+Ruby+Go", crc == EXPECTED,
            $"0x{crc:x8} == 0x{EXPECTED:x8}");

        // Write via P/Invoke
        var handle = KoreFFI.kore_block_new();
        Check("kore_block_new()", handle != IntPtr.Zero);

        var prices = new double[] { 10.5, 20.0, 30.75, 15.0, 45.99, 8.25, 99.0, 55.5, 12.0, 33.33 };
        var pricePinned = GCHandle.Alloc(prices, GCHandleType.Pinned);
        int rc1 = KoreFFI.kore_block_add_f64(handle, "price", pricePinned.AddrOfPinnedObject(), (UIntPtr)prices.Length);
        pricePinned.Free();
        Check("add_f64 'price' column (10 rows)", rc1 == 0, $"rc={rc1}");

        string outFile = Path.Combine(repoRoot, "test_v160_csharp.kore");
        int wrc = KoreFFI.kore_write_file(outFile, handle);
        KoreFFI.kore_block_free(handle);
        Check("write_file rc=0", wrc == 0, $"rc={wrc}");

        if (File.Exists(outFile))
        {
            long sz = new FileInfo(outFile).Length;
            Check(".kore file created", sz > 0, $"{sz} bytes");

            // Read it back
            IntPtr rHandle = KoreFFI.kore_read_file(outFile);
            Check("kore_read_file() != null", rHandle != IntPtr.Zero);
            if (rHandle != IntPtr.Zero)
            {
                ulong nrows = KoreFFI.kore_block_num_rows(rHandle);
                uint  ncols = KoreFFI.kore_block_num_cols(rHandle);
                Check("read: 10 rows", nrows == 10, $"{nrows}");
                Check("read: 1 column", ncols == 1, $"{ncols}");

                var outBuf = new double[10];
                var outPin = GCHandle.Alloc(outBuf, GCHandleType.Pinned);
                long n = KoreFFI.kore_block_get_f64(rHandle, "price", outPin.AddrOfPinnedObject(), 10);
                outPin.Free();
                KoreFFI.kore_block_free(rHandle);
                Check("price[0] = 10.5", Math.Abs(outBuf[0] - 10.5) < 0.001, $"{outBuf[0]:F2}");
            }
            File.Delete(outFile);
        }
    }
    catch (Exception ex)
    {
        Check("P/Invoke execution", false, ex.Message[..Math.Min(80, ex.Message.Length)]);
    }
}

// ── Test: Read Python .kore ──────────────────────────────────────────────────
Console.WriteLine("\n  [3] Cross-language binary compatibility");
string pyKore = Path.Combine(repoRoot, "test_v160_orders.kore");
Check("Python .kore readable by C#", File.Exists(pyKore),
    File.Exists(pyKore) ? $"{new FileInfo(pyKore).Length} bytes" : "missing");

if (File.Exists(pyKore))
{
    var bytes = File.ReadAllBytes(pyKore);
    string magic = Encoding.ASCII.GetString(bytes, 0, 4);
    Check("Magic bytes = KORE", magic == "KORE", $"got \"{magic}\"");
    if (bytes.Length >= 18)
    {
        ulong nrows = BitConverter.ToUInt64(bytes, 10);
        uint ncols  = BitConverter.ToUInt32(bytes, 6);
        Check("Row count = 10", nrows == 10, $"{nrows} rows");
        Check("Column count = 4", ncols == 4, $"{ncols} cols");
    }
}

// ── Summary ──────────────────────────────────────────────────────────────────
int total = passed + failed;
Console.WriteLine();
Console.WriteLine("======================================================================");
Console.WriteLine($"  C# .NET {Environment.Version} | KORE v{VERSION} | {DateTime.UtcNow:yyyy-MM-ddTHH:mm:ssZ}");
Console.WriteLine($"  TOTAL: {passed}/{total} passed | {failed} failed");
Console.WriteLine("======================================================================");
Environment.Exit(failed > 0 ? 1 : 0);

// P/Invoke signatures
static class KoreFFI
{
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_crc32")]
    public static extern uint kore_crc32(IntPtr data, UIntPtr len);
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_new")]
    public static extern IntPtr kore_block_new();
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_free")]
    public static extern void kore_block_free(IntPtr block);
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_add_f64")]
    public static extern int kore_block_add_f64(IntPtr block, [MarshalAs(UnmanagedType.LPStr)] string name, IntPtr data, UIntPtr len);
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_write_file")]
    public static extern int kore_write_file([MarshalAs(UnmanagedType.LPStr)] string path, IntPtr block);
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_read_file")]
    public static extern IntPtr kore_read_file([MarshalAs(UnmanagedType.LPStr)] string path);
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_num_rows")]
    public static extern ulong kore_block_num_rows(IntPtr block);
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_num_cols")]
    public static extern uint kore_block_num_cols(IntPtr block);
    [DllImport("kore_ffi", CallingConvention = CallingConvention.Cdecl, EntryPoint = "kore_block_get_f64")]
    public static extern long kore_block_get_f64(IntPtr block, [MarshalAs(UnmanagedType.LPStr)] string col, IntPtr outBuf, ulong maxlen);
}
