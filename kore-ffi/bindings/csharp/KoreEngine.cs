// KoreEngine.cs — C# / .NET 7+ P/Invoke bindings for the KORE engine.
//
// Build / usage:
//   1. cargo build --release -p kore-ffi
//   2. Copy kore_ffi.dll / libkore_ffi.so beside your .NET executable
//      (or set KORE_LIB env var to the full path)
//   3. dotnet run
//
// Example:
//   using var engine = new KoreEngine();
//   using var block  = engine.NewBlock();
//   block.AddF64("score", new[] { 1.0, 2.0, 3.0 });
//   using var model  = engine.NewModel(ModelType.LinearRegressor);
//   model.Fit(X, nRows, nCols, y);
//   double[] preds = model.Predict(X, nRows, nCols);

using System;
using System.Runtime.InteropServices;

namespace Kore;

// ── Model type enum ───────────────────────────────────────────────────────────

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

// ── P/Invoke declarations ─────────────────────────────────────────────────────

internal static partial class Native
{
    private const string Lib = "kore_ffi";   // OS resolves .dll / .so / .dylib

    [LibraryImport(Lib)] internal static partial IntPtr kore_last_error();
    [LibraryImport(Lib)] internal static partial IntPtr kore_block_new();
    [LibraryImport(Lib)] internal static partial void   kore_block_free(IntPtr ptr);
    [LibraryImport(Lib)] internal static partial ulong  kore_block_num_rows(IntPtr ptr);
    [LibraryImport(Lib)] internal static partial uint   kore_block_num_cols(IntPtr ptr);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int kore_block_add_f64(IntPtr ptr, string name, [In] double[] data, ulong len);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int kore_block_add_i64(IntPtr ptr, string name, [In] long[] data, ulong len);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial long kore_block_get_f64(IntPtr ptr, string col, [Out] double[] out_, ulong maxLen);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial IntPtr kore_hash_join(IntPtr left, IntPtr right,
        string lk, string rk, int joinType);

    [LibraryImport(Lib)]
    internal static partial IntPtr kore_model_new(int type, int p1, int p2);
    [LibraryImport(Lib)] internal static partial void kore_model_free(IntPtr ptr);

    [LibraryImport(Lib)]
    internal static partial int kore_model_fit(IntPtr ptr,
        [In] double[] xFlat, ulong nRows, ulong nCols, [In] double[] y);

    [LibraryImport(Lib)]
    internal static partial int kore_model_predict(IntPtr ptr,
        [In] double[] xFlat, ulong nRows, ulong nCols, [Out] double[] output);
}

// ── Error helper ──────────────────────────────────────────────────────────────

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

// ── KoreBlock ─────────────────────────────────────────────────────────────────

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
        long read = Native.kore_block_get_f64(_ptr, col, buf, (ulong)buf.Length);
        if (read < 0) throw new InvalidOperationException(KoreError.Last());
        return buf[..(int)read];
    }

    public KoreBlock HashJoin(KoreBlock right, string lk, string rk, int how = 0) =>
        new(KoreError.CheckPtr(Native.kore_hash_join(_ptr, right._ptr, lk, rk, how)));

    public void Dispose() { if (_ptr != IntPtr.Zero) { Native.kore_block_free(_ptr); _ptr = IntPtr.Zero; } }
    public override string ToString() => $"KoreBlock(rows={NumRows}, cols={NumCols})";
}

// ── KoreModel ─────────────────────────────────────────────────────────────────

public sealed class KoreModel : IDisposable
{
    private IntPtr _ptr;

    public KoreModel(ModelType type, int param1 = 100, int param2 = 3)
    {
        _ptr = KoreError.CheckPtr(Native.kore_model_new((int)type, param1, param2));
    }

    /// <param name="xFlat">Row-major double[] of length nRows × nCols</param>
    public void Fit(double[] xFlat, ulong nRows, ulong nCols, double[] y) =>
        KoreError.Check(Native.kore_model_fit(_ptr, xFlat, nRows, nCols, y));

    public double[] Predict(double[] xFlat, ulong nRows, ulong nCols)
    {
        var out_ = new double[(int)nRows];
        KoreError.Check(Native.kore_model_predict(_ptr, xFlat, nRows, nCols, out_));
        return out_;
    }

    public void Dispose() { if (_ptr != IntPtr.Zero) { Native.kore_model_free(_ptr); _ptr = IntPtr.Zero; } }
}

// ── KoreEngine (factory helper) ───────────────────────────────────────────────

public sealed class KoreEngine : IDisposable
{
    public KoreBlock NewBlock() => new();
    public KoreModel NewModel(ModelType type, int p1 = 100, int p2 = 3) => new(type, p1, p2);
    public void Dispose() { }   // stateless; each object manages its own memory
}
