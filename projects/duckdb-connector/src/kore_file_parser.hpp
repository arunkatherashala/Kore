#pragma once

#include <vector>
#include <cstdint>
#include <string>

namespace kore {

/**
 * Binary file parser utilities for Kore format.
 */
class KoreFileParser {
public:
    /**
     * Reads a variable-length integer (LEB128 encoding).
     *
     * @param data Pointer to data buffer
     * @param offset Reference to current offset (modified)
     * @return Decoded integer value
     */
    static uint64_t ReadVarInt(const uint8_t *data, size_t &offset);

    /**
     * Reads a 32-bit little-endian integer.
     *
     * @param data Pointer to data buffer
     * @param offset Reference to current offset (modified)
     * @return Decoded integer value
     */
    static uint32_t ReadLE32(const uint8_t *data, size_t &offset);

    /**
     * Reads a 64-bit little-endian integer.
     *
     * @param data Pointer to data buffer
     * @param offset Reference to current offset (modified)
     * @return Decoded integer value
     */
    static uint64_t ReadLE64(const uint8_t *data, size_t &offset);

    /**
     * Reads a UTF-8 string with length prefix.
     *
     * @param data Pointer to data buffer
     * @param offset Reference to current offset (modified)
     * @return Decoded string
     */
    static std::string ReadString(const uint8_t *data, size_t &offset);

    /**
     * Decompresses data using specified codec.
     *
     * @param compressed Compressed data buffer
     * @param codec_id Codec identifier
     * @return Decompressed data
     */
    static std::vector<uint8_t> Decompress(
        const std::vector<uint8_t> &compressed,
        uint8_t codec_id
    );

    /**
     * Validates Kore file magic bytes.
     *
     * @param data Pointer to first 4 bytes
     * @return True if magic bytes match "KORE"
     */
    static bool ValidateMagic(const uint8_t *data);
};

} // namespace kore
