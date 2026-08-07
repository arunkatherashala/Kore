package com.github.arunkatherashala.kore;

/**
 * Enumeration of all supported data types in KORE format.
 * Matches Rust DType enum for format compatibility.
 */
public enum DataType {
    I64(1, "i64"),
    F64(2, "f64"),
    BOOL(3, "bool"),
    STR(4, "str"),
    STR_DICT(5, "str_dict"),
    ARRAY(6, "array"),
    STRUCT(7, "struct");

    public final int code;
    public final String name;

    DataType(int code, String name) {
        this.code = code;
        this.name = name;
    }

    public static DataType fromCode(int code) {
        for (DataType dt : values()) {
            if (dt.code == code) return dt;
        }
        throw new IllegalArgumentException("Unknown DataType code: " + code);
    }
}
