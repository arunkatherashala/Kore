package com.github.arunkatherashala.kore.spark;

import com.github.arunkatherashala.kore.DataBlock;
import com.github.arunkatherashala.kore.ColumnData;
import com.github.arunkatherashala.kore.DataType;
import com.github.arunkatherashala.kore.KoreReader;
import org.apache.spark.sql.types.*;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;

/** Maps Kore DataType → Spark DataType and infers schema from .kore files. */
public class KoreSparkUtils {

    public static StructType inferSchema(String path) {
        try {
            byte[] data = Files.readAllBytes(Paths.get(path));
            DataBlock block = KoreReader.fromBytes(data);
            List<StructField> fields = new ArrayList<>();

            for (ColumnData col : block.getColumns()) {
                fields.add(new StructField(col.getName(), toSparkType(col.getType()), true, Metadata.empty()));
            }
            return new StructType(fields.toArray(new StructField[0]));
        } catch (IOException e) {
            throw new RuntimeException("Cannot infer schema from: " + path, e);
        }
    }

    static org.apache.spark.sql.types.DataType toSparkType(DataType koreType) {
        switch (koreType) {
            case I64:      return DataTypes.LongType;
            case F64:      return DataTypes.DoubleType;
            case BOOL:     return DataTypes.BooleanType;
            case STR:
            case STR_DICT: return DataTypes.StringType;
            default:       return DataTypes.StringType;
        }
    }
}
