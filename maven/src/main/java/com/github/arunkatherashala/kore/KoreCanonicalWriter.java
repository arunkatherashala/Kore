package com.github.arunkatherashala.kore;

import com.github.luben.zstd.Zstd;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;

/**
 * Canonical KORE v2 writer — byte-compatible with the Rust {@code kore-store}
 * reader (and the Python converter), unlike {@link KoreWriter} whose column
 * encoding diverges and cannot represent NULLs.
 *
 * <p>Each column is wrapped in Zstd (codec 6); the wrapped payload is
 * {@code [inner_codec:1] + zstd_frame(block)}, matching the Rust
 * {@code decode_column} Zstd path. Inner codecs:
 * <ul>
 *   <li>i64  → Delta   (2): {@code [null_tag:1][i64_delta:8]} per row</li>
 *   <li>f64  → NaN-raw (4): {@code [f64:8]} per row, NaN = NULL</li>
 *   <li>bool → Raw     (0): 1 byte/row (0=false, 1=true, 2=null)</li>
 *   <li>str  → Raw     (0): {@code [count:u32][null_flags:n][offsets:(n+1)u32][bytes]}</li>
 * </ul>
 */
public final class KoreCanonicalWriter {

    private static final byte[] MAGIC = {'K', 'O', 'R', 'E'};
    private static final short VERSION = 2;
    private static final int DT_I64 = 1, DT_F64 = 2, DT_BOOL = 3, DT_STR = 4;
    private static final int C_RAW = 0, C_DELTA = 2, C_NANRAW = 4, C_ZSTD = 6;

    private final List<String> names = new ArrayList<>();
    private final List<Integer> dtypes = new ArrayList<>();
    private final List<byte[]> innerBlocks = new ArrayList<>();
    private final List<Integer> innerCodecs = new ArrayList<>();
    private long numRows = -1;
    private int zstdLevel = 19;

    /** Set the Zstd compression level (default 19). */
    public KoreCanonicalWriter zstdLevel(int level) {
        this.zstdLevel = level;
        return this;
    }

    /** Add an int64 column. {@code nulls[i] == true} marks row i NULL (nulls may be null). */
    public KoreCanonicalWriter addLong(String name, long[] values, boolean[] nulls) {
        checkRows(values.length);
        ByteArrayOutputStream b = new ByteArrayOutputStream(values.length * 9);
        long prev = 0;
        for (int i = 0; i < values.length; i++) {
            boolean isNull = nulls != null && nulls[i];
            b.write(isNull ? 1 : 0);
            long val = isNull ? 0 : values[i];
            writeLongLE(b, val - prev);          // Java long subtraction wraps like Rust wrapping_sub
            if (!isNull) prev = val;
        }
        add(name, DT_I64, C_DELTA, b.toByteArray());
        return this;
    }

    /** Add a float64 column. {@code nulls[i] == true} marks row i NULL (nulls may be null). */
    public KoreCanonicalWriter addDouble(String name, double[] values, boolean[] nulls) {
        checkRows(values.length);
        ByteArrayOutputStream b = new ByteArrayOutputStream(values.length * 8);
        for (int i = 0; i < values.length; i++) {
            boolean isNull = nulls != null && nulls[i];
            double v = isNull ? Double.NaN : values[i];
            writeLongLE(b, Double.doubleToRawLongBits(v));
        }
        add(name, DT_F64, C_NANRAW, b.toByteArray());
        return this;
    }

    /** Add a boolean column. {@code nulls[i] == true} marks row i NULL (nulls may be null). */
    public KoreCanonicalWriter addBool(String name, boolean[] values, boolean[] nulls) {
        checkRows(values.length);
        ByteArrayOutputStream b = new ByteArrayOutputStream(values.length);
        for (int i = 0; i < values.length; i++) {
            if (nulls != null && nulls[i]) b.write(2);
            else b.write(values[i] ? 1 : 0);
        }
        add(name, DT_BOOL, C_RAW, b.toByteArray());
        return this;
    }

    /** Add a string column. A {@code null} element marks that row NULL. */
    public KoreCanonicalWriter addString(String name, String[] values) {
        checkRows(values.length);
        int n = values.length;
        ByteArrayOutputStream b = new ByteArrayOutputStream();
        writeIntLE(b, n);
        for (String v : values) b.write(v == null ? 1 : 0);   // null flags
        int offset = 0;
        List<byte[]> blobs = new ArrayList<>(n);
        ByteArrayOutputStream offsets = new ByteArrayOutputStream((n + 1) * 4);
        for (String v : values) {
            byte[] bytes = v == null ? new byte[0] : v.getBytes(StandardCharsets.UTF_8);
            writeIntLE(offsets, offset);
            offset += bytes.length;
            blobs.add(bytes);
        }
        writeIntLE(offsets, offset);   // sentinel
        b.write(offsets.toByteArray(), 0, offsets.size());
        for (byte[] blob : blobs) b.write(blob, 0, blob.length);
        add(name, DT_STR, C_RAW, b.toByteArray());
        return this;
    }

    /** Serialize all columns to the KORE v2 byte layout. */
    public byte[] toBytes() {
        if (numRows < 0) numRows = 0;
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        try {
            out.write(MAGIC);
            writeShortLE(out, VERSION);
            writeIntLE(out, names.size());
            writeLongLE(out, numRows);
            for (int i = 0; i < names.size(); i++) {
                byte[] nb = names.get(i).getBytes(StandardCharsets.UTF_8);
                writeShortLE(out, (short) nb.length);
                out.write(nb);
                out.write(dtypes.get(i));
            }
            for (int i = 0; i < names.size(); i++) {
                byte[] inner = innerBlocks.get(i);
                byte[] payload = new byte[1 + (int) Zstd.compressBound(inner.length)];
                payload[0] = (byte) (int) innerCodecs.get(i);
                long csize = Zstd.compressByteArray(payload, 1, payload.length - 1,
                        inner, 0, inner.length, zstdLevel);
                int total = 1 + (int) csize;
                out.write(C_ZSTD);
                writeLongLE(out, total);
                out.write(payload, 0, total);
            }
        } catch (IOException e) {
            throw new RuntimeException("KORE serialize failed", e);
        }
        return out.toByteArray();
    }

    /** Write all columns to a {@code .kore} file. */
    public void writeFile(String path) {
        try {
            Files.write(Paths.get(path), toBytes());
        } catch (IOException e) {
            throw new RuntimeException("KORE write failed: " + path, e);
        }
    }

    private void add(String name, int dtype, int codec, byte[] block) {
        names.add(name);
        dtypes.add(dtype);
        innerCodecs.add(codec);
        innerBlocks.add(block);
    }

    private void checkRows(long len) {
        if (numRows < 0) numRows = len;
        else if (numRows != len) {
            throw new IllegalArgumentException("column length " + len + " != numRows " + numRows);
        }
    }

    private static void writeShortLE(OutputStream o, short v) throws IOException {
        o.write(v & 0xFF);
        o.write((v >> 8) & 0xFF);
    }

    private static void writeIntLE(OutputStream o, int v) {
        try {
            o.write(v & 0xFF);
            o.write((v >> 8) & 0xFF);
            o.write((v >> 16) & 0xFF);
            o.write((v >> 24) & 0xFF);
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }

    private static void writeLongLE(OutputStream o, long v) {
        try {
            for (int s = 0; s < 64; s += 8) o.write((int) ((v >> s) & 0xFF));
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }
}
