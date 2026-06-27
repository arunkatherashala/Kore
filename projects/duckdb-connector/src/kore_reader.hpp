#pragma once

#include "duckdb.hpp"
#include <string>
#include <vector>
#include <cstdint>

using namespace duckdb;

namespace kore {

/**
 * File schema information for Kore format.
 */
struct FileSchema {
    std::vector<std::string> names;
    std::vector<LogicalType> types;
};

/**
 * Column metadata from Kore file.
 */
struct ColumnMetadata {
    uint8_t codec_id;
    uint64_t data_offset;
    uint64_t compressed_size;
    uint64_t uncompressed_size;
};

/**
 * Kore file reader for DuckDB.
 * Handles binary parsing and chunk reading of Kore format.
 */
class KoreReader {
public:
    /**
     * Creates a new reader for a Kore file.
     *
     * @param filename Path to the Kore file
     */
    explicit KoreReader(const std::string &filename);

    /**
     * Destructor - closes file handle.
     */
    ~KoreReader();

    /**
     * Reads file header and column metadata.
     * Must be called before ReadChunk.
     *
     * @throws std::runtime_error If file is invalid
     */
    void ReadHeader();

    /**
     * Reads the next chunk of data.
     *
     * @param output Output DataChunk
     * @param chunk_size Maximum rows to read
     */
    void ReadChunk(DataChunk &output, size_t chunk_size);

    /**
     * Gets the file schema.
     *
     * @return Const reference to schema
     */
    const FileSchema &GetSchema() const;

    /**
     * Gets total row count.
     *
     * @return Number of rows in file
     */
    uint64_t GetRowCount() const;

    /**
     * Gets column count.
     *
     * @return Number of columns
     */
    uint32_t GetColumnCount() const;

private:
    std::string filename;
    FILE *file;
    FileSchema schema;
    std::vector<ColumnMetadata> column_metadata;
    
    uint64_t current_row;
    uint64_t total_rows;
    uint32_t column_count;
};

/**
 * Bind data for read_kore function.
 */
struct KoreBindData : public FunctionData {
    std::string filename;
    std::unique_ptr<KoreReader> reader;
    size_t chunk_size;

    unique_ptr<FunctionData> Copy() const override {
        auto result = make_unique<KoreBindData>();
        result->filename = filename;
        result->chunk_size = chunk_size;
        return move(result);
    }
};

/**
 * Global state for read_kore function.
 */
struct KoreGlobalState : public GlobalFunctionData {
    size_t total_rows_read = 0;

    unique_ptr<GlobalFunctionData> Copy() const override {
        auto result = make_unique<KoreGlobalState>();
        result->total_rows_read = total_rows_read;
        return move(result);
    }
};

/**
 * Local state for read_kore function.
 */
struct KoreLocalState : public LocalFunctionData {
    std::unique_ptr<KoreReader> reader;

    unique_ptr<LocalFunctionData> Copy() const override {
        auto result = make_unique<KoreLocalState>();
        return move(result);
    }
};

} // namespace kore
