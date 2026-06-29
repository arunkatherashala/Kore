package com.kore;

import java.lang.foreign.*;
import java.lang.invoke.*;
import java.nio.file.*;
import java.util.*;

/**
 * KoreEngine — Java 21 + Panama (java.lang.foreign) bindings for the KORE engine.
 *
 * Requires: JDK 21+, libkore_ffi built with:
 *   cargo build --release -p kore-ffi
 *
 * Usage:
 *   try (var kore = new KoreEngine()) {
 *       long block = kore.blockNew();
 *       kore.blockAddF64(block, "score", new double[]{1.0, 2.0, 3.0});
 *       long model = kore.modelNew(KoreEngine.ModelType.LINEAR_REGRESSOR, 0, 0);
 *       kore.modelFit(model, X_flat, nRows, nCols, y);
 *       double[] preds = kore.modelPredict(model, X_flat, nRows, nCols);
 *   }
 */
public class KoreEngine implements AutoCloseable {

    // ── Model type constants ──────────────────────────────────────────────────
    public static final class ModelType {
        public static final int RF_REGRESSOR     = 0;
        public static final int RF_CLASSIFIER    = 1;
        public static final int GBM_REGRESSOR    = 2;
        public static final int LINEAR_REGRESSOR = 3;
        public static final int LOGISTIC         = 4;
        public static final int KNN_REGRESSOR    = 5;
        public static final int KNN_CLASSIFIER   = 6;
        public static final int SVM              = 7;
    }

    private final Arena    arena;
    private final Linker   linker;
    private final SymbolLookup lookup;

    // Method handles cached at construction time
    private final MethodHandle mh_block_new;
    private final MethodHandle mh_block_free;
    private final MethodHandle mh_block_num_rows;
    private final MethodHandle mh_block_num_cols;
    private final MethodHandle mh_block_add_f64;
    private final MethodHandle mh_block_add_i64;
    private final MethodHandle mh_block_get_f64;
    private final MethodHandle mh_hash_join;
    private final MethodHandle mh_model_new;
    private final MethodHandle mh_model_free;
    private final MethodHandle mh_model_fit;
    private final MethodHandle mh_model_predict;
    private final MethodHandle mh_last_error;

    public KoreEngine() {
        this(findLibPath());
    }

    public KoreEngine(String libPath) {
        this.arena  = Arena.ofShared();
        this.linker = Linker.nativeLinker();
        this.lookup = SymbolLookup.libraryLookup(libPath, arena);

        mh_block_new      = link("kore_block_new",  FunctionDescriptor.of(ValueLayout.ADDRESS));
        mh_block_free     = link("kore_block_free", FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
        mh_block_num_rows = link("kore_block_num_rows", FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
        mh_block_num_cols = link("kore_block_num_cols", FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS));
        mh_block_add_f64  = link("kore_block_add_f64", FunctionDescriptor.of(ValueLayout.JAVA_INT,
            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));
        mh_block_add_i64  = link("kore_block_add_i64", FunctionDescriptor.of(ValueLayout.JAVA_INT,
            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));
        mh_block_get_f64  = link("kore_block_get_f64", FunctionDescriptor.of(ValueLayout.JAVA_LONG,
            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));
        mh_hash_join      = link("kore_hash_join", FunctionDescriptor.of(ValueLayout.ADDRESS,
            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
        mh_model_new      = link("kore_model_new", FunctionDescriptor.of(ValueLayout.ADDRESS,
            ValueLayout.JAVA_INT, ValueLayout.JAVA_INT, ValueLayout.JAVA_INT));
        mh_model_free     = link("kore_model_free", FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
        mh_model_fit      = link("kore_model_fit", FunctionDescriptor.of(ValueLayout.JAVA_INT,
            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
        mh_model_predict  = link("kore_model_predict", FunctionDescriptor.of(ValueLayout.JAVA_INT,
            ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS));
        mh_last_error     = link("kore_last_error", FunctionDescriptor.of(ValueLayout.ADDRESS));
    }

    private MethodHandle link(String name, FunctionDescriptor desc) {
        return linker.downcallHandle(lookup.find(name).orElseThrow(
            () -> new RuntimeException("Symbol not found: " + name)), desc);
    }

    // ── DataBlock API ─────────────────────────────────────────────────────────

    public long blockNew() {
        try { return ((MemorySegment) mh_block_new.invoke()).address(); }
        catch (Throwable e) { throw new RuntimeException(e); }
    }

    public void blockFree(long ptr) {
        try { mh_block_free.invoke(MemorySegment.ofAddress(ptr)); }
        catch (Throwable e) { throw new RuntimeException(e); }
    }

    public long blockNumRows(long ptr) {
        try { return (long) mh_block_num_rows.invoke(MemorySegment.ofAddress(ptr)); }
        catch (Throwable e) { throw new RuntimeException(e); }
    }

    public int blockNumCols(long ptr) {
        try { return (int) mh_block_num_cols.invoke(MemorySegment.ofAddress(ptr)); }
        catch (Throwable e) { throw new RuntimeException(e); }
    }

    public void blockAddF64(long ptr, String col, double[] data) {
        try (var tmp = Arena.ofConfined()) {
            var nameSeg = tmp.allocateFrom(col);
            var dataSeg = tmp.allocateFrom(ValueLayout.JAVA_DOUBLE, data);
            int rc = (int) mh_block_add_f64.invoke(
                MemorySegment.ofAddress(ptr), nameSeg, dataSeg, (long) data.length);
            if (rc != 0) throw new RuntimeException("kore_block_add_f64 failed: " + lastError());
        } catch (Throwable e) { throw new RuntimeException(e); }
    }

    public void blockAddI64(long ptr, String col, long[] data) {
        try (var tmp = Arena.ofConfined()) {
            var nameSeg = tmp.allocateFrom(col);
            var dataSeg = tmp.allocateFrom(ValueLayout.JAVA_LONG, data);
            int rc = (int) mh_block_add_i64.invoke(
                MemorySegment.ofAddress(ptr), nameSeg, dataSeg, (long) data.length);
            if (rc != 0) throw new RuntimeException("kore_block_add_i64 failed: " + lastError());
        } catch (Throwable e) { throw new RuntimeException(e); }
    }

    public double[] blockGetF64(long ptr, String col) {
        long n = blockNumRows(ptr);
        try (var tmp = Arena.ofConfined()) {
            var colSeg = tmp.allocateFrom(col);
            var outSeg = tmp.allocate(ValueLayout.JAVA_DOUBLE, n);
            long read  = (long) mh_block_get_f64.invoke(
                MemorySegment.ofAddress(ptr), colSeg, outSeg, n);
            if (read < 0) throw new RuntimeException("kore_block_get_f64: " + lastError());
            double[] result = new double[(int) read];
            for (int i = 0; i < read; i++) result[i] = outSeg.getAtIndex(ValueLayout.JAVA_DOUBLE, i);
            return result;
        } catch (Throwable e) { throw new RuntimeException(e); }
    }

    // ── HashJoin ──────────────────────────────────────────────────────────────

    /** how: 0=INNER 1=LEFT 2=FULL. Returns handle to new block (caller must free). */
    public long hashJoin(long left, long right, String lk, String rk, int how) {
        try (var tmp = Arena.ofConfined()) {
            var lkSeg = tmp.allocateFrom(lk);
            var rkSeg = tmp.allocateFrom(rk);
            MemorySegment res = (MemorySegment) mh_hash_join.invoke(
                MemorySegment.ofAddress(left), MemorySegment.ofAddress(right),
                lkSeg, rkSeg, how);
            if (res.address() == 0) throw new RuntimeException("hash_join: " + lastError());
            return res.address();
        } catch (Throwable e) { throw new RuntimeException(e); }
    }

    // ── ML Models ─────────────────────────────────────────────────────────────

    public long modelNew(int type, int p1, int p2) {
        try {
            MemorySegment ptr = (MemorySegment) mh_model_new.invoke(type, p1, p2);
            if (ptr.address() == 0) throw new RuntimeException("model_new: " + lastError());
            return ptr.address();
        } catch (Throwable e) { throw new RuntimeException(e); }
    }

    public void modelFree(long ptr) {
        try { mh_model_free.invoke(MemorySegment.ofAddress(ptr)); }
        catch (Throwable e) { throw new RuntimeException(e); }
    }

    /** x_flat: row-major double[], length = nRows * nCols */
    public void modelFit(long ptr, double[] x_flat, long nRows, long nCols, double[] y) {
        try (var tmp = Arena.ofConfined()) {
            var xSeg = tmp.allocateFrom(ValueLayout.JAVA_DOUBLE, x_flat);
            var ySeg = tmp.allocateFrom(ValueLayout.JAVA_DOUBLE, y);
            int rc = (int) mh_model_fit.invoke(
                MemorySegment.ofAddress(ptr), xSeg, nRows, nCols, ySeg);
            if (rc != 0) throw new RuntimeException("model_fit: " + lastError());
        } catch (Throwable e) { throw new RuntimeException(e); }
    }

    public double[] modelPredict(long ptr, double[] x_flat, long nRows, long nCols) {
        try (var tmp = Arena.ofConfined()) {
            var xSeg   = tmp.allocateFrom(ValueLayout.JAVA_DOUBLE, x_flat);
            var outSeg = tmp.allocate(ValueLayout.JAVA_DOUBLE, nRows);
            int rc = (int) mh_model_predict.invoke(
                MemorySegment.ofAddress(ptr), xSeg, nRows, nCols, outSeg);
            if (rc != 0) throw new RuntimeException("model_predict: " + lastError());
            double[] result = new double[(int) nRows];
            for (int i = 0; i < nRows; i++) result[i] = outSeg.getAtIndex(ValueLayout.JAVA_DOUBLE, i);
            return result;
        } catch (Throwable e) { throw new RuntimeException(e); }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    public String lastError() {
        try {
            MemorySegment seg = (MemorySegment) mh_last_error.invoke();
            return seg.address() == 0 ? null : seg.reinterpret(1024).getString(0);
        } catch (Throwable e) { return e.getMessage(); }
    }

    @Override public void close() { arena.close(); }

    private static String findLibPath() {
        String env = System.getenv("KORE_LIB");
        if (env != null) return env;
        String os  = System.getProperty("os.name").toLowerCase();
        String ext  = os.contains("win") ? ".dll" : os.contains("mac") ? ".dylib" : ".so";
        String pre  = os.contains("win") ? "" : "lib";
        Path   root = Path.of(System.getProperty("user.dir"));
        // try to find target/release relative to cwd or parent dirs
        for (int i = 0; i < 5; i++) {
            Path p = root.resolve("target/release/" + pre + "kore_ffi" + ext);
            if (Files.exists(p)) return p.toString();
            Path parent = root.getParent();
            if (parent == null) break;
            root = parent;
        }
        throw new RuntimeException(
            "libkore_ffi not found. Build with: cargo build --release -p kore-ffi"
        );
    }
}
