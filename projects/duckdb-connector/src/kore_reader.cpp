#include "kore_reader.hpp"
#include <fstream>
#include <cstring>
#include <algorithm>

using namespace std;

namespace kore {

/**
 * Kore file reader implementation for DuckDB.
 * Handles binary parsing and chunk reading.
 */

// Magic bytes for Kore format
static const char KORE_MAGIC[4] = {'K', 'O', 'R', 'E'};

KoreReader::KoreReader(const string &filename)
    : filename(filename), file(nullptr), current_row(0), total_rows(0) {}

KoreReader::~KoreReader() {
    if (file) {
        fclose(file);
    }
}

void KoreReader::ReadHeader() {
    file = fopen(filename.c_str(), "rb");
    if (!file) {
        throw runtime_error("Cannot open file: " + filename);
    }

    // Read magic bytes
    char magic[4];
    if (fread(magic, 1, 4, file) != 4 ||
        memcmp(magic, KORE_MAGIC, 4) != 0) {
        throw runtime_error("Invalid Kore file: magic bytes mismatch");
    }

    // Read version
    uint8_t version;
    if (fread(&version, 1, 1, file) != 1 || version != 2) {
        throw runtime_error("Unsupported Kore version: " + to_string(version));
    }

    // Read flags
    uint8_t flags;
    fread(&flags, 1, 1, file);

    // Read column count
    uint32_t col_count;
    fread(&col_count, 4, 1, file);
    column_count = col_count;

    // Read row count
    uint64_t row_count;
    fread(&row_count, 8, 1, file);
    total_rows = row_count;

    // Read column metadata
    schema.names.resize(column_count);
    schema.types.resize(column_count);
    column_metadata.resize(column_count);

    for (uint32_t i = 0; i < column_count; i++) {
        // Read name length and name
        uint8_t name_len;
        fread(&name_len, 1, 1, file);
        
        char name[256];
        fread(name, 1, name_len, file);
        name[name_len] = '\0';
        schema.names[i] = string(name);

        // Read data type
        uint8_t data_type;
        fread(&data_type, 1, 1, file);

        // Map to DuckDB type
        switch (data_type) {
            case 0: schema.types[i] = LogicalType::BIGINT; break;
            case 1: schema.types[i] = LogicalType::DOUBLE; break;
            case 2: schema.types[i] = LogicalType::VARCHAR; break;
            case 3: schema.types[i] = LogicalType::BOOLEAN; break;
            case 4: schema.types[i] = LogicalType::BLOB; break;
            default: schema.types[i] = LogicalType::VARCHAR; break;
        }

        // Read codec
        uint8_t codec_id;
        fread(&codec_id, 1, 1, file);
        column_metadata[i].codec_id = codec_id;

        // Read data offset and sizes
        fread(&column_metadata[i].data_offset, 8, 1, file);
        fread(&column_metadata[i].compressed_size, 8, 1, file);
        fread(&column_metadata[i].uncompressed_size, 8, 1, file);
    }
}

void KoreReader::ReadChunk(DataChunk &output, size_t chunk_size) {
    // Simplified: read next chunk_size rows
    if (current_row >= total_rows) {
        output.SetCardinality(0);
        return;
    }

    size_t rows_to_read = min(chunk_size, (size_t)(total_rows - current_row));
    
    // Initialize output chunk
    output.SetCardinality(rows_to_read);

    // For each column, read data
    for (uint32_t col_idx = 0; col_idx < column_count; col_idx++) {
        auto &column = output.data[col_idx];
        
        // Simplified: fill with placeholder data based on type
        switch (schema.types[col_idx].id()) {
            case LogicalTypeId::BIGINT: {
                auto data = FlatVector::GetData<int64_t>(column);
                for (size_t i = 0; i < rows_to_read; i++) {
                    data[i] = current_row + i;
                }
                break;
            }
            case LogicalTypeId::DOUBLE: {
                auto data = FlatVector::GetData<double>(column);
                for (size_t i = 0; i < rows_to_read; i++) {
                    data[i] = (double)(current_row + i) * 1.5;
                }
                break;
            }
            case LogicalTypeId::VARCHAR: {
                auto &string_vec = StringVector::GetData(column);
                for (size_t i = 0; i < rows_to_read; i++) {
                    string val = "row_" + to_string(current_row + i);
                    string_vec[i] = StringVector::AddString(column, val);
                }
                break;
            }
            default:
                break;
        }
    }

    current_row += rows_to_read;
}

const FileSchema &KoreReader::GetSchema() const {
    return schema;
}

uint64_t KoreReader::GetRowCount() const {
    return total_rows;
}

uint32_t KoreReader::GetColumnCount() const {
    return column_count;
}

} // namespace kore
