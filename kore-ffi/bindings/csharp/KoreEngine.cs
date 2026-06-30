// KoreEngine.cs -- C# / .NET 7+ P/Invoke bindings for the KORE engine.
//
// Covers:
//   * DataBlock / ML API   (KoreBlock, KoreModel, KoreEngine)
//   * SQL Session API      (KoreSession)
//
// Build / usage:
//   1. cargo build --release -p kore-ffi
//   2. Copy kore_ffi.dll / libkore_ffi.so beside your .NET executable
//      (or set KORE_LIB env var to the full path)
//   3. dotnet run
//
// Example:
//   using var block = new KoreBlock();
//   block.AddF64("x", new[] { 1.0, 2.0, 3.0 });
//
//   using var sess = new KoreSession();
//   sess.LoadCsv("sales", "/data/sales.csv");
//   var rows = sess.Query("SELECT region, SUM(amount) FROM sales GROUP BY region");

using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace Kore;

// =============================================================================
// Model type enum
// =============================================================================

public enum ModelType : int
{
    RfRegressor     = 0,
    RfClassifier    = 1,
    GbmRegressor    = 2,
    LinearRegressor = 3,
    Logistic        = 4,
    KnnRegressor    = 5,
    KnnClassifier   = 6,
    Svm             = 7,
}

// =============================================================================
// P/Invoke declarations
// =============================================================================

internal static partial class Native
{
    private const string Lib = "kore_ffi";   // OS resolves .dll / .so / .dylib

    // -- Error
    [LibraryImport(Lib)] internal static partial IntPtr kore_last_error();

    // -- DataBlock
    [LibraryImport(Lib)] internal static partial IntPtr kore_block_new();
    [LibraryImport(Lib)] internal static partial void   kore_block_free(IntPtr ptr);
    [LibraryImport(Lib)] internal static partial ulong  kore_block_num_rows(IntPtr ptr);
    [LibraryImport(Lib)] internal static partial uint   kore_block_num_cols(IntPtr ptr);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int kore_block_add_f64(IntPtr ptr, string name,
        [In] double[] data, ulong len);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int kore_block_add_i64(IntPtr ptr, string name,
        [In] long[] data, ulong len);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial long kore_block_get_f64(IntPtr ptr, string col,
        [Out] double[] buf, ulong maxLen);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial IntPtr kore_hash_join(IntPtr left, IntPtr right,
        string lk, string rk, int joinType);

    // -- ML Models
    [LibraryImport(Lib)]
    internal static partial IntPtr kore_model_new(int type, int p1, int p2);
    [LibraryImport(Lib)] internal static partial void kore_model_free(IntPtr ptr);

    [LibraryImport(Lib)]
    internal static partial int kore_model_fit(IntPtr ptr,
        [In] double[] xFlat, ulong nRows, ulong nCols, [In] double[] y);

    [LibraryImport(Lib)]
    internal static partial int kore_model_predict(IntPtr ptr,
        [In] double[] xFlat, ulong nRows, ulong nCols, [Out] double[] output);

    // -- SQL Session
    [LibraryImport(Lib)] internal static partial IntPtr kore_session_new();
    [LibraryImport(Lib)] internal static partial void   kore_session_free(IntPtr sess);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int kore_session_load_csv(IntPtr sess,
        string tableName, string path);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int kore_session_register_block(IntPtr sess,
        string tableName, IntPtr block);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial IntPtr kore_session_query(IntPtr sess, string sql);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial long kore_session_row_count(IntPtr sess, string tableName);

    [LibraryImport(Lib)] internal static partial void kore_free_string(IntPtr s);
}

// =============================================================================
// Error helper
// =============================================================================

internal static class KoreError
{
    internal static string? Last()
    {
        IntPtr p = Native.kore_last_error();
        return p == IntPtr.Zero ? null : Marshal.PtrToStringUTF8(p);
    }

    internal static void Check(int rc)
    {
        if (rc != 0)
            throw new InvalidOperationException(Last() ?? $"KORE error code {rc}");
    }

    internal static IntPtr CheckPtr(IntPtr ptr)
    {
        if (ptr == IntPtr.Zero)
            throw new InvalidOperationException(Last() ?? "KORE returned null pointer");
        return ptr;
    }
}

// =============================================================================
// KoreBlock
// =============================================================================

public sealed class KoreBlock : IDisposable
{
    private IntPtr _ptr;
    internal IntPtr Ptr => _ptr;

    public KoreBlock()
    {
        _ptr = KoreError.CheckPtr(Native.kore_block_new());
    }

    internal KoreBlock(IntPtr ptr) { _ptr = ptr; }

    public ulong NumRows => Native.kore_block_num_rows(_ptr);
    public uint  NumCols => Native.kore_block_num_cols(_ptr);

    public void AddF64(string name, double[] data) =>
        KoreError.Check(Native.kore_block_add_f64(_ptr, name, data, (ulong)data.Length));

    public void AddI64(string name, long[] data) =>
        KoreError.Check(Native.kore_block_add_i64(_ptr, name, data, (ulong)data.Length));

    public double[] GetF64(string col)
    {
        var buf  = new double[NumRows];
        long n   = Native.kore_block_get_f64(_ptr, col, buf, (ulong)buf.Length);
        if (n < 0) throw new InvalidOperationException(KoreError.Last());
        return buf[..(int)n];
    }

    public KoreBlock HashJoin(KoreBlock right, string lk, string rk, int how = 0) =>
        new(KoreError.CheckPtr(Native.kore_hash_join(_ptr, right._ptr, lk, rk, how)));

    public void Dispose()
    {
        if (_ptr != IntPtr.Zero) { Native.kore_block_free(_ptr); _ptr = IntPtr.Zero; }
    }

    public override string ToString() => $"KoreBlock(rows={NumRows}, cols={NumCols})";
}

// =============================================================================
// KoreModel
// =============================================================================

public sealed class KoreModel : IDisposable
{
    private IntPtr _ptr;

    public KoreModel(ModelType type, int param1 = 100, int param2 = 3)
    {
        _ptr = KoreError.CheckPtr(Native.kore_model_new((int)type, param1, param2));
    }

    /// <param name="xFlat">Row-major double[] of length nRows x nCols.</param>
    public void Fit(double[] xFlat, ulong nRows, ulong nCols, double[] y) =>
        KoreError.Check(Native.kore_model_fit(_ptr, xFlat, nRows, nCols, y));

    public double[] Predict(double[] xFlat, ulong nRows, ulong nCols)
    {
        var out_ = new double[(int)nRows];
        KoreError.Check(Native.kore_model_predict(_ptr, xFlat, nRows, nCols, out_));
        return out_;
    }

    public void Dispose()
    {
        if (_ptr != IntPtr.Zero) { Native.kore_model_free(_ptr); _ptr = IntPtr.Zero; }
    }
}

// =============================================================================
// KoreSession
// =============================================================================

/// <summary>
/// High-level SQL session backed by KORE's in-memory query engine.
/// Each instance is an independent in-memory database.
/// </summary>
public sealed class KoreSession : IDisposable
{
    private IntPtr _handle;

    public KoreSession()
    {
        _handle = KoreError.CheckPtr(Native.kore_session_new());
    }

    // -------------------------------------------------------------------------
    // Data loading
    // -------------------------------------------------------------------------

    /// <summary>Load a CSV file on disk as a named table.</summary>
    public void LoadCsv(string table, string path)
    {
        path = System.IO.Path.GetFullPath(path);
        KoreError.Check(Native.kore_session_load_csv(_handle, table, path));
    }

    /// <summary>
    /// Load a list of row dictionaries as a named table via a temporary CSV.
    /// </summary>
    public void LoadTable(string table, IReadOnlyList<Dictionary<string, object?>> rows)
    {
        if (rows == null || rows.Count == 0)
            throw new ArgumentException("rows must not be empty", nameof(rows));

        var cols = new List<string>(rows[0].Keys);
        var tmp  = System.IO.Path.Combine(System.IO.Path.GetTempPath(),
                       $"kore_{Guid.NewGuid():N}.csv");
        try
        {
            using var sw = new StreamWriter(tmp, false, System.Text.Encoding.UTF8);
            sw.WriteLine(string.Join(",", cols));
            foreach (var row in rows)
            {
                var fields = new List<string>(cols.Count);
                foreach (var col in cols)
                {
                    var v = row.TryGetValue(col, out var val) ? val?.ToString() ?? "" : "";
                    fields.Add(v.Contains(',') || v.Contains('"')
                        ? $"\"{v.Replace("\"", "\"\"")}\"" : v);
                }
                sw.WriteLine(string.Join(",", fields));
            }
            LoadCsv(table, tmp);
        }
        finally
        {
            if (File.Exists(tmp)) File.Delete(tmp);
        }
    }

    /// <summary>Register a KoreBlock as a named table (data is copied).</summary>
    public void RegisterBlock(string table, KoreBlock block) =>
        KoreError.Check(Native.kore_session_register_block(_handle, table, block.Ptr));

    // -------------------------------------------------------------------------
    // Query
    // -------------------------------------------------------------------------

    /// <summary>
    /// Execute a SQL query and return results as a list of dictionaries.
    /// </summary>
    public List<Dictionary<string, object?>> Query(string sql)
    {
        IntPtr raw = Native.kore_session_query(_handle, sql);
        if (raw == IntPtr.Zero)
            throw new InvalidOperationException(KoreError.Last() ?? $"Query returned NULL: {sql}");

        string jsonStr;
        try   { jsonStr = Marshal.PtrToStringUTF8(raw) ?? "[]"; }
        finally { Native.kore_free_string(raw); }

        var result = new List<Dictionary<string, object?>>();
        using var doc = JsonDocument.Parse(jsonStr);
        foreach (var elem in doc.RootElement.EnumerateArray())
        {
            var row = new Dictionary<string, object?>();
            foreach (var prop in elem.EnumerateObject())
            {
                row[prop.Name] = prop.Value.ValueKind switch
                {
                    JsonValueKind.Number => prop.Value.TryGetInt64(out long l) ? (object)l : prop.Value.GetDouble(),
                    JsonValueKind.String => prop.Value.GetString(),
                    JsonValueKind.True   => true,
                    JsonValueKind.False  => false,
                    JsonValueKind.Null   => null,
                    _                   => prop.Value.ToString(),
                };
            }
            result.Add(row);
        }
        return result;
    }

    // -------------------------------------------------------------------------
    // Metadata
    // -------------------------------------------------------------------------

    /// <summary>Return the row count of a named table.</summary>
    public long RowCount(string table)
    {
        long n = Native.kore_session_row_count(_handle, table);
        if (n < 0) throw new KeyNotFoundException($"Table '{table}' not found");
        return n;
    }

    // -------------------------------------------------------------------------
    // Lifecycle
    // -------------------------------------------------------------------------

    public void Dispose()
    {
        if (_handle != IntPtr.Zero)
        {
            Native.kore_session_free(_handle);
            _handle = IntPtr.Zero;
        }
    }

    public override string ToString() =>
        $"KoreSession(handle={_handle})";
}

// =============================================================================
// KoreEngine (factory helper -- backwards-compatible with existing code)
// =============================================================================

public sealed class KoreEngine : IDisposable
{
    public KoreBlock   NewBlock()                                   => new();
    public KoreModel   NewModel(ModelType t, int p1 = 100, int p2 = 3) => new(t, p1, p2);
    public KoreSession NewSession()                                 => new();
    public void Dispose() { }   // stateless; each object manages its own memory
}

// =============================================================================
// Demo
// =============================================================================

// Uncomment and run with: dotnet script KoreEngine.cs
// (or add to a .csproj and run dotnet run)
/*
using Kore;

using var sess = new KoreSession();
sess.LoadTable("products", new List<Dictionary<string, object?>>
{
    new() { ["id"] = 1L, ["name"] = "Widget",    ["price"] = 9.99  },
    new() { ["id"] = 2L, ["name"] = "Gadget",    ["price"] = 24.99 },
    new() { ["id"] = 3L, ["name"] = "Doohickey", ["price"] = 4.49  },
});
Console.WriteLine($"Row count: {sess.RowCount("products")}");
var rows = sess.Query("SELECT * FROM products ORDER BY price DESC");
foreach (var r in rows) Console.WriteLine(string.Join(", ", r));
var agg = sess.Query("SELECT SUM(price) AS total FROM products");
Console.WriteLine($"Total: {agg[0]["total"]}");
*/