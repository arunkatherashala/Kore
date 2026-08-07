# frozen_string_literal: true

require 'minitest/autorun'
require_relative 'kore_fileformat'

class TestKoreDataTypes < Minitest::Test
  def test_data_type_values
    assert_equal 1, Kore::FileFormat::DataType::I64
    assert_equal 2, Kore::FileFormat::DataType::F64
    assert_equal 3, Kore::FileFormat::DataType::BOOL
    assert_equal 4, Kore::FileFormat::DataType::STR
    assert_equal 5, Kore::FileFormat::DataType::STR_DICT
    assert_equal 6, Kore::FileFormat::DataType::ARRAY
    assert_equal 7, Kore::FileFormat::DataType::STRUCT
  end

  def test_compression_values
    assert_equal 0, Kore::FileFormat::Compression::RAW
    assert_equal 1, Kore::FileFormat::Compression::RLE
    assert_equal 2, Kore::FileFormat::Compression::DELTA
    assert_equal 3, Kore::FileFormat::Compression::DICT
    assert_equal 4, Kore::FileFormat::Compression::NAN_RAW
    assert_equal 5, Kore::FileFormat::Compression::DEFLATE
    assert_equal 6, Kore::FileFormat::Compression::ZSTD
  end
end

class TestKoreDataBlock < Minitest::Test
  def test_create_empty_data_block
    block = Kore::FileFormat::DataBlock.new

    assert_equal 0, block.num_rows
    assert_equal 0, block.num_columns
    assert_empty block.columns
  end

  def test_add_single_column
    block = Kore::FileFormat::DataBlock.new
    block.add_column('numbers', Kore::FileFormat::DataType::I64, [1, 2, 3, 4, 5])

    assert_equal 5, block.num_rows
    assert_equal 1, block.num_columns
    assert_not_nil block.get_column('numbers')
  end

  def test_add_multiple_columns
    block = Kore::FileFormat::DataBlock.new
    block.add_column('numbers', Kore::FileFormat::DataType::I64, [1, 2, 3])
    block.add_column('names', Kore::FileFormat::DataType::STR, %w[a b c])

    assert_equal 3, block.num_rows
    assert_equal 2, block.num_columns
  end

  def test_add_column_mismatched_rows_raises
    block = Kore::FileFormat::DataBlock.new
    block.add_column('numbers', Kore::FileFormat::DataType::I64, [1, 2, 3])

    error = assert_raises(ArgumentError) do
      block.add_column('names', Kore::FileFormat::DataType::STR, %w[a b])
    end

    assert_match(/has 2 rows, expected 3/, error.message)
  end

  def test_get_column_by_name
    block = Kore::FileFormat::DataBlock.new
    block.add_column('test', Kore::FileFormat::DataType::I64, [10, 20, 30])

    col = block.get_column('test')
    assert_not_nil col
    assert_equal 'test', col.name
    assert_equal Kore::FileFormat::DataType::I64, col.dtype
    assert_equal [10, 20, 30], col.data
  end

  def test_get_nonexistent_column
    block = Kore::FileFormat::DataBlock.new
    block.add_column('test', Kore::FileFormat::DataType::I64, [1, 2, 3])

    col = block.get_column('nonexistent')
    assert_nil col
  end
end

class TestKoreColumnStats < Minitest::Test
  def test_create_column_stats
    stats = Kore::FileFormat::ColumnStats.new(
      min_value: 1,
      max_value: 100,
      null_count: 0,
      cardinality: 50,
      crc32: 0xdeadbeef
    )

    assert_equal 1, stats.min_value
    assert_equal 100, stats.max_value
    assert_equal 0, stats.null_count
    assert_equal 50, stats.cardinality
    assert_equal 0xdeadbeef, stats.crc32
  end

  def test_column_stats_to_hash
    stats = Kore::FileFormat::ColumnStats.new(
      min_value: 5,
      max_value: 95,
      cardinality: 20
    )

    hash = stats.to_h
    assert_equal 5, hash[:min_value]
    assert_equal 95, hash[:max_value]
    assert_equal 20, hash[:cardinality]
  end
end

class TestKoreColumn < Minitest::Test
  def test_create_column
    data = [1, 2, 3]
    col = Kore::FileFormat::Column.new('test', Kore::FileFormat::DataType::I64, data)

    assert_equal 'test', col.name
    assert_equal Kore::FileFormat::DataType::I64, col.dtype
    assert_equal data, col.data
    assert_nil col.stats
  end

  def test_column_with_stats
    stats = Kore::FileFormat::ColumnStats.new(min_value: 1, max_value: 3)
    col = Kore::FileFormat::Column.new('test', Kore::FileFormat::DataType::I64, [1, 2, 3], stats)

    assert_equal stats, col.stats
  end

  def test_column_to_hash
    col = Kore::FileFormat::Column.new('test', Kore::FileFormat::DataType::I64, [1, 2])

    hash = col.to_h
    assert_equal 'test', hash[:name]
    assert_equal Kore::FileFormat::DataType::I64, hash[:type]
    assert_equal [1, 2], hash[:data]
  end
end

class TestKoreVersionSnapshot < Minitest::Test
  def test_create_version_snapshot
    version = Kore::FileFormat::VersionSnapshot.new(
      version_id: 1,
      timestamp: 1234567890,
      block_offset: 100,
      row_count: 1000
    )

    assert_equal 1, version.version_id
    assert_equal 1234567890, version.timestamp
    assert_equal 100, version.block_offset
    assert_equal 1000, version.row_count
    assert_nil version.prev_version
  end

  def test_create_version_snapshot_with_previous
    version = Kore::FileFormat::VersionSnapshot.new(
      version_id: 2,
      timestamp: 1234567900,
      block_offset: 200,
      row_count: 2000,
      prev_version: 1
    )

    assert_equal 2, version.version_id
    assert_equal 1, version.prev_version
  end
end

class TestKorePartitionSpec < Minitest::Test
  def test_create_partition_spec
    spec = Kore::FileFormat::PartitionSpec.new(
      spec_id: 1,
      columns: [0, 1],
      transforms: %w[year month]
    )

    assert_equal 1, spec.spec_id
    assert_equal [0, 1], spec.columns
    assert_equal %w[year month], spec.transforms
  end
end

class TestKoreDeleteVector < Minitest::Test
  def test_create_delete_vector
    dv = Kore::FileFormat::DeleteVector.new(
      bitmap: "\xff\x00",
      cardinality: 8,
      timestamp: 1234567890
    )

    assert_equal "\xff\x00", dv.bitmap
    assert_equal 8, dv.cardinality
    assert_equal 1234567890, dv.timestamp
  end
end

class TestKoreVersion < Minitest::Test
  def test_version_constant
    assert_equal '2.0.0', Kore::FileFormat::VERSION
  end
end

class TestKoreWriteFile < Minitest::Test
  def test_write_file_json_fallback
    require 'tempfile'
    require 'json'

    block = Kore::FileFormat::DataBlock.new
    block.add_column('numbers', Kore::FileFormat::DataType::I64, [1, 2, 3])

    Tempfile.create do |f|
      Kore.write_file(f.path, block)

      content = File.read(f.path)
      data = JSON.parse(content)

      assert_equal 3, data['num_rows']
      assert_equal 1, data['num_columns']
      assert_equal 'numbers', data['columns'][0]['name']
    end
  end
end

# Phase 3 placeholder tests
class TestKorePhaseThree < Minitest::Test
  def test_crc32_pending
    skip('Phase 3: CRC32 FFI pending')
    Kore.crc32(b'test')
  end

  def test_read_file_pending
    skip('Phase 3: Binary format reading pending')
    Kore.read_file('/tmp/test.kore')
  end

  def test_encrypt_decrypt_pending
    skip('Phase 3: Encryption API pending')
    Kore.encrypt_aes256('password', b'data')
  end

  def test_get_column_stats_pending
    skip('Phase 3: Stats API pending')
    Kore.get_column_stats(b'', 'column_name')
  end

  def test_get_bloom_filter_pending
    skip('Phase 3: Bloom filter API pending')
    Kore.get_bloom_filter(b'', 'column_name')
  end
end
