package com.github.arunkatherashala.kore;

import com.github.luben.zstd.Zstd;
import com.google.gson.JsonObject;
import com.google.gson.JsonArray;

import java.util.zip.Deflater;
import java.util.zip.DeflaterOutputStream;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.List;

/**
 * KORE Writer: Serialize DataBlock to KORE format with all 11 features.
 * 
 * Format:
 * [MAGIC (4)] [VERSION (2)] [NUM_COLS (4)] [NUM_ROWS (8)]
 * [SCHEMA...] [COL_COMPRESSION (1)] [COL_LEN (8)] [COL_DATA...]... 
 * [FOOTER_LEN (8)] [FOOTER_JSON] [TRAILER]
 */
public class KoreWriter {
    private static final byte[] MAGIC = {'K', 'O', 'R', 'E'};
    private static final short VERSION = 2;

    /**
     * Serialize a DataBlock to KORE format bytes.
     * @param block DataBlock to serialize
     * @return KORE-formatted bytes with human-readable trailer
     */
    public static byte[] toBytes(DataBlock block) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        List<ColumnData> columns = block.getColumns();
        long numRows = block.getNumRows();

        try {
            // ── Header ────────────────────────────────────────────
            out.write(MAGIC);
            writeShort(out, VERSION);
            writeInt(out, columns.size());
            writeLong(out, numRows);

            // ── Schema ────────────────────────────────────────────
            for (ColumnData col : columns) {
                writeString(out, col.getName());
                out.write(col.getType().code);
            }

            // ── Column Data (with compression + stats + checksums) ────
            List<byte[]> compressedColumns = new ArrayList<>();
            List<ColumnData.ColumnStats> allStats = new ArrayList<>();

            for (ColumnData col : columns) {
                // Encode column
                byte[] rawData = encodeColumn(col, numRows);
                long crc = Checksums.crc32(rawData);

                // For now: use RAW compression (no-op) to verify format
                // TODO: Add dual-compression picker (LZ4 vs ZSTD) after format validation
                byte[] compressedData = rawData;
                Compression codec = Compression.RAW;

                compressedColumns.add(compressedData);

                // Compute statistics (Feature 2)
                ColumnData.ColumnStats stats = computeStats(col, crc);
                allStats.add(stats);
            }

            // Write compressed column data
            for (int i = 0; i < columns.size(); i++) {
                byte[] data = compressedColumns.get(i);
                out.write(Compression.RAW.code); // Write actual codec used
                writeLong(out, data.length);
                out.write(data);
            }

            // ── Footer (human-readable JSON trailer) ────────────────
            JsonObject footer = new JsonObject();
            footer.addProperty("magic", new String(MAGIC));
            footer.addProperty("version", VERSION);
            footer.addProperty("num_rows", numRows);
            footer.addProperty("num_cols", columns.size());

            JsonArray statsArray = new JsonArray();
            for (int i = 0; i < columns.size(); i++) {
                JsonObject colStats = new JsonObject();
                colStats.addProperty("name", columns.get(i).getName());
                colStats.addProperty("type", columns.get(i).getType().name);
                ColumnData.ColumnStats stats = allStats.get(i);
                if (stats != null) {
                    if (stats.minValue != null) {
                        colStats.addProperty("min", stats.minValue);
                        colStats.addProperty("max", stats.maxValue);
                    } else {
                        colStats.addProperty("min", stats.minValueF);
                        colStats.addProperty("max", stats.maxValueF);
                    }
                    colStats.addProperty("nulls", stats.nullCount);
                    colStats.addProperty("cardinality", stats.cardinality);
                    colStats.addProperty("crc32", stats.crc32);
                }
                statsArray.add(colStats);
            }
            footer.add("column_stats", statsArray);

            byte[] footerJson = footer.toString().getBytes();
            byte[] binaryData = out.toByteArray();

            // Final output: binary + footer length + footer JSON
            ByteArrayOutputStream finalOut = new ByteArrayOutputStream();
            finalOut.write(binaryData);
            writeLong(finalOut, footerJson.length);
            finalOut.write(footerJson);

            // Add readable trailer for debugging
            String trailer = "\n// ─── KORE Format v2 (Human-Readable Trailer) ───\n" +
                "// " + footer.toString() + "\n";
            finalOut.write(trailer.getBytes());

            return finalOut.toByteArray();
        } catch (IOException e) {
            throw new RuntimeException("Write failed", e);
        }
    }

    /**
     * Write DataBlock to file in KORE format.
     * @param block DataBlock to serialize
     * @param path File path to write
     */
    public static void toFile(DataBlock block, String path) {
        try (FileOutputStream fos = new FileOutputStream(new File(path))) {
            fos.write(toBytes(block));
        } catch (IOException e) {
            throw new RuntimeException("File write failed: " + path, e);
        }
    }

    // ── Helper methods ────────────────────────────────────────────

    private static byte[] encodeColumn(ColumnData col, long numRows) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        Object data = col.getData();

        switch (col.getType()) {
            case I64:
                long[] i64Data = (long[]) data;
                for (long v : i64Data) writeLong(out, v);
                break;
            case F64:
                double[] f64Data = (double[]) data;
                for (double v : f64Data) writeDouble(out, v);
                break;
            case BOOL:
                boolean[] boolData = (boolean[]) data;
                for (boolean v : boolData) out.write(v ? 1 : 0);
                break;
            case STR:
            case STR_DICT:
                List<String> strData = (List<String>) data;
                for (String s : strData) writeString(out, s);
                break;
            case ARRAY:
            case STRUCT:
                // Nested types: placeholder
                out.write(0);
                break;
        }

        return out.toByteArray();
    }

    private static ColumnData.ColumnStats computeStats(ColumnData col, long crc) {
        Object data = col.getData();
        long nullCount = 0;
        long cardinality = 0;

        try {
            if (col.getType() == DataType.I64) {
                long[] values = (long[]) data;
                long min = Long.MAX_VALUE, max = Long.MIN_VALUE;
                for (long v : values) {
                    if (v == 0) nullCount++; // Simplified null detection
                    min = Math.min(min, v);
                    max = Math.max(max, v);
                }
                cardinality = values.length - nullCount;
                return new ColumnData.ColumnStats(min, max, nullCount, cardinality, crc);
            } else if (col.getType() == DataType.F64) {
                double[] values = (double[]) data;
                double min = Double.MAX_VALUE, max = -Double.MAX_VALUE;
                for (double v : values) {
                    min = Math.min(min, v);
                    max = Math.max(max, v);
                }
                cardinality = values.length - nullCount;
                return new ColumnData.ColumnStats(min, max, nullCount, cardinality, crc);
            }
        } catch (Exception e) {
            // Stats computation failed, return null
        }

        return null;
    }

    private static byte[] tryLz4Compress(byte[] data) {
        // Use Java's Deflater as a lightweight compressor (similar to LZ4)
        try {
            Deflater deflater = new Deflater(6);
            deflater.setInput(data);
            deflater.finish();
            byte[] compressed = new byte[data.length];
            int compressedSize = deflater.deflate(compressed);
            deflater.end();
            if (compressedSize > 0) {
                byte[] result = new byte[compressedSize];
                System.arraycopy(compressed, 0, result, 0, compressedSize);
                return result;
            }
        } catch (Exception e) {
            // Compression failed, fall back to raw
        }
        return null;
    }

    private static byte[] tryZstdCompress(byte[] data) {
        try {
            return Zstd.compress(data);
        } catch (Exception e) {
            return null;
        }
    }

    private static void writeShort(ByteArrayOutputStream out, short v) {
        out.write((v >> 0) & 0xFF);
        out.write((v >> 8) & 0xFF);
    }

    private static void writeInt(ByteArrayOutputStream out, int v) {
        out.write((v >> 0) & 0xFF);
        out.write((v >> 8) & 0xFF);
        out.write((v >> 16) & 0xFF);
        out.write((v >> 24) & 0xFF);
    }

    private static void writeLong(ByteArrayOutputStream out, long v) {
        out.write((int) ((v >> 0) & 0xFF));
        out.write((int) ((v >> 8) & 0xFF));
        out.write((int) ((v >> 16) & 0xFF));
        out.write((int) ((v >> 24) & 0xFF));
        out.write((int) ((v >> 32) & 0xFF));
        out.write((int) ((v >> 40) & 0xFF));
        out.write((int) ((v >> 48) & 0xFF));
        out.write((int) ((v >> 56) & 0xFF));
    }

    private static void writeDouble(ByteArrayOutputStream out, double v) {
        writeLong(out, Double.doubleToLongBits(v));
    }

    private static void writeString(ByteArrayOutputStream out, String s) {
        byte[] bytes = s.getBytes();
        writeShort(out, (short) bytes.length);
        try {
            out.write(bytes);
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }
}
