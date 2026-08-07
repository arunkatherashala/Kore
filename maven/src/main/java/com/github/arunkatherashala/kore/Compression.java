package com.github.arunkatherashala.kore;

/**
 * Enumeration of compression codecs supported by KORE.
 * Matches Rust Compression enum for format compatibility.
 */
public enum Compression {
    RAW(0, "raw"),
    RLE(1, "rle"),
    DELTA(2, "delta"),
    DICT(3, "dict"),
    NAN_RAW(4, "nan_raw"),
    LZ4(5, "lz4"),
    ZSTD(6, "zstd");

    public final int code;
    public final String name;

    Compression(int code, String name) {
        this.code = code;
        this.name = name;
    }

    public static Compression fromCode(int code) {
        for (Compression c : values()) {
            if (c.code == code) return c;
        }
        throw new IllegalArgumentException("Unknown Compression code: " + code);
    }
}
