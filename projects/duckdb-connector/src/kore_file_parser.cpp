#include "kore_file_parser.hpp"
#include <cstring>
#include <stdexcept>

namespace kore {

/**
 * Implementation of Kore file parser utilities.
 */

uint64_t KoreFileParser::ReadVarInt(const uint8_t *data, size_t &offset) {
    uint64_t result = 0;
    int shift = 0;

    while (true) {
        uint8_t byte = data[offset++];
        result |= ((uint64_t)(byte & 0x7F)) << shift;

        if ((byte & 0x80) == 0) {
            break;
        }

        shift += 7;
        if (shift >= 64) {
            throw std::runtime_error("VarInt overflow");
        }
    }

    return result;
}

uint32_t KoreFileParser::ReadLE32(const uint8_t *data, size_t &offset) {
    uint32_t result =
        ((uint32_t)data[offset]) |
        (((uint32_t)data[offset + 1]) << 8) |
        (((uint32_t)data[offset + 2]) << 16) |
        (((uint32_t)data[offset + 3]) << 24);
    offset += 4;
    return result;
}

uint64_t KoreFileParser::ReadLE64(const uint8_t *data, size_t &offset) {
    uint64_t low = ReadLE32(data, offset);
    uint64_t high = ReadLE32(data, offset);
    return low | (high << 32);
}

std::string KoreFileParser::ReadString(const uint8_t *data, size_t &offset) {
    uint8_t len = data[offset++];
    std::string result((char *)&data[offset], len);
    offset += len;
    return result;
}

std::vector<uint8_t> KoreFileParser::Decompress(
    const std::vector<uint8_t> &compressed,
    uint8_t codec_id
) {
    // Codec IDs:
    // 0 = None (uncompressed)
    // 1 = RLE
    // 2 = Dictionary
    // 3 = FOR
    // 4 = LZSS
    // 5 = EnhancedDictionary
    // 6 = DoubleDelta

    std::vector<uint8_t> result;

    switch (codec_id) {
        case 0: // None
            result = compressed;
            break;

        case 1: // RLE
        case 2: // Dictionary
        case 3: // FOR
        case 4: // LZSS
        case 5: // EnhancedDictionary
        case 6: // DoubleDelta
            // Placeholder: would call actual decompressors
            result = compressed;
            break;

        default:
            throw std::runtime_error("Unknown codec: " + std::to_string(codec_id));
    }

    return result;
}

bool KoreFileParser::ValidateMagic(const uint8_t *data) {
    static const uint8_t MAGIC[4] = {'K', 'O', 'R', 'E'};
    return memcmp(data, MAGIC, 4) == 0;
}

} // namespace kore
