package com.github.arunkatherashala.kore;

import org.apache.hadoop.conf.Configuration;
import org.apache.hadoop.fs.Path;
import org.apache.parquet.example.data.Group;
import org.apache.parquet.hadoop.ParquetFileReader;
import org.apache.parquet.hadoop.ParquetReader;
import org.apache.parquet.hadoop.example.GroupReadSupport;
import org.apache.parquet.hadoop.util.HadoopInputFile;
import org.apache.parquet.io.api.Binary;
import org.apache.parquet.schema.LogicalTypeAnnotation;
import org.apache.parquet.schema.LogicalTypeAnnotation.DateLogicalTypeAnnotation;
import org.apache.parquet.schema.LogicalTypeAnnotation.TimestampLogicalTypeAnnotation;
import org.apache.parquet.schema.MessageType;
import org.apache.parquet.schema.PrimitiveType.PrimitiveTypeName;
import org.apache.parquet.schema.Type;

import java.nio.charset.StandardCharsets;

/**
 * Read an Apache Parquet file and write it as a canonical KORE file via
 * {@link KoreCanonicalWriter}. Output is byte-compatible with the Rust/Python
 * KoreReader and lossless (NULLs preserved).
 *
 * <p>Type mapping (matches the Rust {@code kore-parquet} reader):
 * <ul>
 *   <li>INT32/INT64 → i64; DATE → i64 days; TIMESTAMP/INT96 → i64 microseconds</li>
 *   <li>FLOAT/DOUBLE → f64; BOOLEAN → bool; BINARY(UTF8)/other → str</li>
 * </ul>
 *
 * <p>Requires {@code parquet-hadoop} + {@code hadoop-common} on the classpath.
 */
public final class ParquetToKore {

    private static final long JULIAN_EPOCH = 2440588L;          // Julian day of 1970-01-01
    private static final long MICROS_PER_DAY = 86_400_000_000L;

    private ParquetToKore() {}

    /** Convert {@code parquetPath} to {@code korePath} at the given zstd level. */
    public static void convert(String parquetPath, String korePath, int zstdLevel) throws Exception {
        Configuration conf = new Configuration();
        Path path = new Path(parquetPath);

        MessageType schema;
        int total;
        try (ParquetFileReader fr = ParquetFileReader.open(HadoopInputFile.fromPath(path, conf))) {
            schema = fr.getFooter().getFileMetaData().getSchema();
            total = Math.toIntExact(fr.getRecordCount());
        }

        int nCols = schema.getFieldCount();
        Kind[] kinds = new Kind[nCols];
        long[][] longs = new long[nCols][];
        double[][] doubles = new double[nCols][];
        boolean[][] bools = new boolean[nCols][];
        String[][] strs = new String[nCols][];
        boolean[][] nulls = new boolean[nCols][];

        for (int c = 0; c < nCols; c++) {
            Kind k = classify(schema.getType(c));
            kinds[c] = k;
            nulls[c] = new boolean[total];
            switch (k.kore) {
                case I64:  longs[c]   = new long[total];    break;
                case F64:  doubles[c] = new double[total];  break;
                case BOOL: bools[c]   = new boolean[total]; break;
                default:   strs[c]    = new String[total];  break;
            }
        }

        try (ParquetReader<Group> reader =
                 ParquetReader.builder(new GroupReadSupport(), path).withConf(conf).build()) {
            Group g;
            int r = 0;
            while ((g = reader.read()) != null) {
                for (int c = 0; c < nCols; c++) {
                    if (g.getFieldRepetitionCount(c) == 0) {
                        nulls[c][r] = true;
                        continue;
                    }
                    Kind k = kinds[c];
                    switch (k.kore) {
                        case I64:  longs[c][r]   = readLong(g, c, k);       break;
                        case F64:  doubles[c][r] = readDouble(g, c, k);     break;
                        case BOOL: bools[c][r]   = g.getBoolean(c, 0);      break;
                        default:   strs[c][r]    = readString(g, c);        break;
                    }
                }
                r++;
            }
        }

        KoreCanonicalWriter w = new KoreCanonicalWriter().zstdLevel(zstdLevel);
        for (int c = 0; c < nCols; c++) {
            String name = schema.getType(c).getName();
            switch (kinds[c].kore) {
                case I64:  w.addLong(name, longs[c], nulls[c]);     break;
                case F64:  w.addDouble(name, doubles[c], nulls[c]); break;
                case BOOL: w.addBool(name, bools[c], nulls[c]);     break;
                default:
                    for (int i = 0; i < total; i++) if (nulls[c][i]) strs[c][i] = null;
                    w.addString(name, strs[c]);
                    break;
            }
        }
        w.writeFile(korePath);
    }

    // ── type classification ──────────────────────────────────────────────────

    private enum KoreType { I64, F64, BOOL, STR }

    private static final class Kind {
        final KoreType kore;
        final PrimitiveTypeName phys;
        final Conv conv;
        Kind(KoreType kore, PrimitiveTypeName phys, Conv conv) { this.kore = kore; this.phys = phys; this.conv = conv; }
    }

    private enum Conv { PLAIN, DATE_DAYS, TS_MILLIS, TS_MICROS, TS_NANOS, INT96_TS }

    private static Kind classify(Type field) {
        PrimitiveTypeName p = field.asPrimitiveType().getPrimitiveTypeName();
        LogicalTypeAnnotation lt = field.getLogicalTypeAnnotation();
        switch (p) {
            case BOOLEAN: return new Kind(KoreType.BOOL, p, Conv.PLAIN);
            case FLOAT:
            case DOUBLE:  return new Kind(KoreType.F64, p, Conv.PLAIN);
            case INT96:   return new Kind(KoreType.I64, p, Conv.INT96_TS);
            case INT32:
                if (lt instanceof DateLogicalTypeAnnotation) return new Kind(KoreType.I64, p, Conv.DATE_DAYS);
                return new Kind(KoreType.I64, p, Conv.PLAIN);
            case INT64:
                if (lt instanceof TimestampLogicalTypeAnnotation) {
                    switch (((TimestampLogicalTypeAnnotation) lt).getUnit()) {
                        case MILLIS: return new Kind(KoreType.I64, p, Conv.TS_MILLIS);
                        case NANOS:  return new Kind(KoreType.I64, p, Conv.TS_NANOS);
                        default:     return new Kind(KoreType.I64, p, Conv.TS_MICROS);
                    }
                }
                return new Kind(KoreType.I64, p, Conv.PLAIN);
            default:      return new Kind(KoreType.STR, p, Conv.PLAIN);   // BINARY / FIXED
        }
    }

    private static long readLong(Group g, int c, Kind k) {
        switch (k.conv) {
            case DATE_DAYS: return g.getInteger(c, 0);
            case TS_MILLIS: return g.getLong(c, 0) * 1_000L;
            case TS_MICROS: return g.getLong(c, 0);
            case TS_NANOS:  return g.getLong(c, 0) / 1_000L;
            case INT96_TS:  return int96ToMicros(g.getInt96(c, 0));
            default:        return k.phys == PrimitiveTypeName.INT32 ? g.getInteger(c, 0) : g.getLong(c, 0);
        }
    }

    private static double readDouble(Group g, int c, Kind k) {
        return k.phys == PrimitiveTypeName.FLOAT ? g.getFloat(c, 0) : g.getDouble(c, 0);
    }

    private static String readString(Group g, int c) {
        Binary b = g.getBinary(c, 0);
        return new String(b.getBytes(), StandardCharsets.UTF_8);
    }

    private static long int96ToMicros(Binary int96) {
        byte[] b = int96.getBytes();                 // 12 bytes LE: [nanosOfDay:8][julianDay:4]
        long nanos = 0;
        for (int i = 0; i < 8; i++) nanos |= (b[i] & 0xFFL) << (8 * i);
        int julian = 0;
        for (int i = 0; i < 4; i++) julian |= (b[8 + i] & 0xFF) << (8 * i);
        return (julian - JULIAN_EPOCH) * MICROS_PER_DAY + nanos / 1_000L;
    }

    public static void main(String[] args) throws Exception {
        if (args.length < 2) {
            System.err.println("usage: ParquetToKore <input.parquet> <output.kore> [zstdLevel=19]");
            System.exit(2);
        }
        int level = args.length > 2 ? Integer.parseInt(args[2]) : 19;
        convert(args[0], args[1], level);
        System.out.println("wrote " + args[1]);
    }
}
