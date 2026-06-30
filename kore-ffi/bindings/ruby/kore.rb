# kore.rb -- Ruby bindings for the KORE engine using Fiddle (stdlib).
#
# Covers:
#   * DataBlock / ML API  (Kore::Block, Kore::Model)
#   * SQL Session API     (Kore::Session / KoreSession convenience class)
#
# No gems required -- uses Ruby's built-in Fiddle library.
#
# Build first:
#   cargo build --release -p kore-ffi
#
# Then:
#   ruby kore.rb              # smoke test
#   require_relative 'kore'  # use from other files
#
# Env override:
#   KORE_LIB=/path/to/libkore_ffi.so ruby kore.rb

require 'fiddle'
require 'fiddle/import'
require 'json'
require 'csv'
require 'tempfile'

module Kore
  # ---------------------------------------------------------------------------
  # Low-level FFI layer via Fiddle::Importer
  # ---------------------------------------------------------------------------
  module FFI
    extend Fiddle::Importer

    def self._find_lib
      return ENV['KORE_LIB'] if ENV['KORE_LIB']
      root = File.expand_path('../../../..', __FILE__)
      candidates = [
        File.join(root, 'target', 'release', 'kore_ffi.dll'),
        File.join(root, 'target', 'release', 'libkore_ffi.so'),
        File.join(root, 'target', 'release', 'libkore_ffi.dylib'),
      ]
      found = candidates.find { |p| File.exist?(p) }
      found or raise LoadError,
        "KORE shared library not found.\n" \
        "Build with: cargo build --release -p kore-ffi\n" \
        "Then set KORE_LIB=/path/to/lib"
    end

    dlload _find_lib

    # Error
    extern 'char*   kore_last_error(void)'

    # DataBlock
    extern 'void*   kore_block_new(void)'
    extern 'void    kore_block_free(void*)'
    extern 'unsigned long long kore_block_num_rows(void*)'
    extern 'unsigned int kore_block_num_cols(void*)'
    extern 'int     kore_block_add_f64(void*, char*, double*, unsigned long long)'
    extern 'int     kore_block_add_i64(void*, char*, long long*, unsigned long long)'
    extern 'long long kore_block_get_f64(void*, char*, double*, unsigned long long)'
    extern 'void*   kore_hash_join(void*, void*, char*, char*, int)'

    # ML Models
    extern 'void*   kore_model_new(int, int, int)'
    extern 'void    kore_model_free(void*)'
    extern 'int     kore_model_fit(void*, double*, unsigned long long, unsigned long long, double*)'
    extern 'int     kore_model_predict(void*, double*, unsigned long long, unsigned long long, double*)'

    # SQL Session
    extern 'void*   kore_session_new(void)'
    extern 'void    kore_session_free(void*)'
    extern 'int     kore_session_load_csv(void*, char*, char*)'
    extern 'int     kore_session_register_block(void*, char*, void*)'
    extern 'void*   kore_session_query(void*, char*)'
    extern 'long long kore_session_row_count(void*, char*)'
    extern 'void    kore_free_string(void*)'
  end

  # ---------------------------------------------------------------------------
  # Error helper
  # ---------------------------------------------------------------------------
  def self.check!(rc)
    return if rc == 0 || rc.nil?
    msg = FFI.kore_last_error
    raise RuntimeError, "KORE error: #{msg || "code #{rc}"}"
  end

  def self.check_ptr!(ptr)
    if ptr.nil? || (ptr.respond_to?(:null?) && ptr.null?) || ptr == 0
      msg = FFI.kore_last_error
      raise RuntimeError, "KORE error: #{msg || 'NULL pointer returned'}"
    end
    ptr
  end

  # ---------------------------------------------------------------------------
  # Block
  # ---------------------------------------------------------------------------
  class Block
    RF_REGRESSOR     = 0
    RF_CLASSIFIER    = 1
    GBM_REGRESSOR    = 2
    LINEAR_REGRESSOR = 3
    LOGISTIC         = 4
    KNN_REGRESSOR    = 5
    KNN_CLASSIFIER   = 6
    SVM              = 7

    def initialize(ptr = nil)
      @ptr = ptr || Kore.check_ptr!(FFI.kore_block_new)
      ObjectSpace.define_finalizer(self, self.class.method(:_finalizer).curry.call(@ptr))
    end

    def self._finalizer(ptr, _obj_id)
      FFI.kore_block_free(ptr)
    end

    def free
      return unless @ptr
      FFI.kore_block_free(@ptr)
      @ptr = nil
    end

    def num_rows = FFI.kore_block_num_rows(@ptr)
    def num_cols = FFI.kore_block_num_cols(@ptr)

    # @param data [Array<Float>]
    def add_f64(name, data)
      buf = Fiddle::Pointer.malloc(data.size * Fiddle::SIZEOF_DOUBLE)
      buf[0, data.size * Fiddle::SIZEOF_DOUBLE] = data.pack('d*')
      Kore.check!(FFI.kore_block_add_f64(@ptr, name, buf, data.size))
      self
    end

    # @param data [Array<Integer>]
    def add_i64(name, data)
      buf = Fiddle::Pointer.malloc(data.size * Fiddle::SIZEOF_LONG_LONG)
      buf[0, data.size * Fiddle::SIZEOF_LONG_LONG] = data.pack('q*')
      Kore.check!(FFI.kore_block_add_i64(@ptr, name, buf, data.size))
      self
    end

    # @return [Array<Float>]
    def get_f64(col)
      n   = num_rows
      buf = Fiddle::Pointer.malloc(n * Fiddle::SIZEOF_DOUBLE)
      rc  = FFI.kore_block_get_f64(@ptr, col, buf, n)
      raise "KORE error: #{FFI.kore_last_error}" if rc < 0
      buf[0, rc * Fiddle::SIZEOF_DOUBLE].unpack('d*')
    end

    # @param how [Integer] 0=INNER 1=LEFT 2=FULL
    def hash_join(right, lk, rk, how = 0)
      ptr = FFI.kore_hash_join(@ptr, right.instance_variable_get(:@ptr), lk, rk, how)
      Block.new(Kore.check_ptr!(ptr))
    end

    def to_s = "KoreBlock(rows=#{num_rows}, cols=#{num_cols})"
    def inspect = to_s
  end

  # ---------------------------------------------------------------------------
  # Model
  # ---------------------------------------------------------------------------
  class Model
    RF_REGRESSOR     = 0
    RF_CLASSIFIER    = 1
    GBM_REGRESSOR    = 2
    LINEAR_REGRESSOR = 3
    LOGISTIC         = 4
    KNN_REGRESSOR    = 5
    KNN_CLASSIFIER   = 6
    SVM              = 7

    def initialize(type, param1 = 100, param2 = 3)
      @ptr = Kore.check_ptr!(FFI.kore_model_new(type, param1, param2))
      ObjectSpace.define_finalizer(self, self.class.method(:_finalizer).curry.call(@ptr))
    end

    def self._finalizer(ptr, _obj_id)
      FFI.kore_model_free(ptr)
    end

    # @param x_flat [Array<Float>] row-major, length = n_rows * n_cols
    # @param y      [Array<Float>]
    def fit(x_flat, n_rows, n_cols, y)
      xbuf = Fiddle::Pointer.malloc(x_flat.size * Fiddle::SIZEOF_DOUBLE)
      xbuf[0, x_flat.size * Fiddle::SIZEOF_DOUBLE] = x_flat.pack('d*')
      ybuf = Fiddle::Pointer.malloc(y.size * Fiddle::SIZEOF_DOUBLE)
      ybuf[0, y.size * Fiddle::SIZEOF_DOUBLE] = y.pack('d*')
      Kore.check!(FFI.kore_model_fit(@ptr, xbuf, n_rows, n_cols, ybuf))
      self
    end

    # @return [Array<Float>]
    def predict(x_flat, n_rows, n_cols)
      xbuf = Fiddle::Pointer.malloc(x_flat.size * Fiddle::SIZEOF_DOUBLE)
      xbuf[0, x_flat.size * Fiddle::SIZEOF_DOUBLE] = x_flat.pack('d*')
      obuf = Fiddle::Pointer.malloc(n_rows * Fiddle::SIZEOF_DOUBLE)
      Kore.check!(FFI.kore_model_predict(@ptr, xbuf, n_rows, n_cols, obuf))
      obuf[0, n_rows * Fiddle::SIZEOF_DOUBLE].unpack('d*')
    end
  end

  # ---------------------------------------------------------------------------
  # Session
  # ---------------------------------------------------------------------------
  class Session
    def initialize
      @ptr = Kore.check_ptr!(FFI.kore_session_new)
    end

    # -- Data loading ----------------------------------------------------------

    # Load a CSV file on disk as a named table.
    def load_csv(table, path)
      abs = File.expand_path(path)
      Kore.check!(FFI.kore_session_load_csv(@ptr, table, abs))
      self
    end

    # Load an Array of Hashes as a named table via a temporary CSV.
    # @param data [Array<Hash>]
    def load_table(name, data)
      raise ArgumentError, 'data must not be empty' if data.nil? || data.empty?
      cols = data.first.keys.map(&:to_s)
      Tempfile.create(["kore_", ".csv"]) do |f|
        f.write(CSV.generate_line(cols))
        data.each do |row|
          f.write(CSV.generate_line(cols.map { |c| row[c.to_sym] || row[c] }))
        end
        f.flush
        load_csv(name, f.path)
      end
      self
    end

    # Register a Block as a named SQL table (data is copied).
    def register_block(table, block)
      Kore.check!(FFI.kore_session_register_block(
        @ptr, table, block.instance_variable_get(:@ptr)
      ))
      self
    end

    # -- Query -----------------------------------------------------------------

    # Execute SQL and return Array of Hashes.
    # @return [Array<Hash>]
    def query(sql)
      raw_ptr = FFI.kore_session_query(@ptr, sql)
      if raw_ptr.nil? || (raw_ptr.respond_to?(:null?) && raw_ptr.null?) || raw_ptr == 0
        msg = FFI.kore_last_error
        raise RuntimeError, "KORE query error: #{msg || 'NULL result'}"
      end
      # Read the C string and free the heap buffer
      json_str = Fiddle::Pointer.new(raw_ptr).to_s
      FFI.kore_free_string(raw_ptr)
      JSON.parse(json_str)
    end

    # -- Metadata --------------------------------------------------------------

    def row_count(table)
      n = FFI.kore_session_row_count(@ptr, table)
      raise KeyError, "Table '#{table}' not found" if n < 0
      n
    end

    # -- Lifecycle -------------------------------------------------------------

    def close
      return unless @ptr
      FFI.kore_session_free(@ptr)
      @ptr = nil
    end

    def to_s = "KoreSession(ptr=#{@ptr})"
    def inspect = to_s
  end
end

# Convenience top-level alias
KoreSession = Kore::Session
KoreBlock   = Kore::Block
KoreModel   = Kore::Model

# ---------------------------------------------------------------------------
# Smoke test
# ---------------------------------------------------------------------------
if __FILE__ == $0
  puts "=== KORE Ruby bindings smoke test ===\n\n"

  puts "1. DataBlock API"
  blk = Kore::Block.new
  blk.add_f64('x', [1.0, 2.0, 3.0, 4.0])
  blk.add_i64('id', [10, 20, 30, 40])
  puts "   #{blk}"
  puts "   x column: #{blk.get_f64('x').inspect}"

  puts "\n2. ML Model (LinearRegressor)"
  model = Kore::Model.new(Kore::Model::LINEAR_REGRESSOR)
  x_flat = [1.0, 2.0, 3.0, 4.0, 5.0]
  y      = [2.0, 4.0, 6.0, 8.0, 10.0]
  model.fit(x_flat, 5, 1, y)
  preds = model.predict([6.0, 7.0], 2, 1)
  puts "   Predictions for x=6,7: #{preds.inspect}"

  puts "\n3. SQL Session API"
  sess = Kore::Session.new
  sess.load_table('sales', [
    { region: 'North', amount: 1000.0 },
    { region: 'South', amount: 2000.0 },
    { region: 'North', amount: 500.0  },
  ])
  puts "   row_count('sales') = #{sess.row_count('sales')}"
  puts "   SUM by region: #{sess.query('SELECT region, SUM(amount) AS total FROM sales GROUP BY region').inspect}"
  puts "   WHERE amount > 600: #{sess.query('SELECT * FROM sales WHERE amount > 600').inspect}"

  puts "\n4. register_block -> SQL"
  sess2 = Kore::Session.new
  sess2.register_block('blk', blk)
  puts "   SUM(x): #{sess2.query('SELECT SUM(x) AS s FROM blk').inspect}"

  sess.close
  sess2.close
  puts "\nAll tests passed."
end