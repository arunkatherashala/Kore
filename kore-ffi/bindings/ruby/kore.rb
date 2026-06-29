# kore.rb — Ruby FFI bindings for the KORE engine.
#
# Install:  gem install ffi
# Usage:
#   require_relative 'kore'
#   block = Kore::Block.new
#   block.add_f64('score', [1.0, 2.0, 3.0])
#   model = Kore::Model.new(Kore::Model::LINEAR_REGRESSOR)
#   model.fit(x_flat, n_rows, n_cols, y)
#   preds = model.predict(x_flat, n_rows, n_cols)

require 'ffi'

module Kore
  extend FFI::Library

  def self.lib_path
    return ENV['KORE_LIB'] if ENV['KORE_LIB']
    root = File.expand_path('../../../..', __FILE__)
    candidates = [
      File.join(root, 'target/release/kore_ffi.dll'),
      File.join(root, 'target/release/libkore_ffi.so'),
      File.join(root, 'target/release/libkore_ffi.dylib'),
    ]
    candidates.find { |p| File.exist?(p) } ||
      raise("libkore_ffi not found. Build with: cargo build --release -p kore-ffi")
  end

  ffi_lib lib_path

  # ── FFI declarations ────────────────────────────────────────────────────────
  attach_function :kore_last_error,     [],                           :string
  attach_function :kore_block_new,      [],                           :pointer
  attach_function :kore_block_free,     [:pointer],                   :void
  attach_function :kore_block_num_rows, [:pointer],                   :uint64
  attach_function :kore_block_num_cols, [:pointer],                   :uint32
  attach_function :kore_block_add_f64,  [:pointer, :string, :pointer, :uint64], :int
  attach_function :kore_block_add_i64,  [:pointer, :string, :pointer, :uint64], :int
  attach_function :kore_block_get_f64,  [:pointer, :string, :pointer, :uint64], :int64
  attach_function :kore_hash_join,      [:pointer, :pointer, :string, :string, :int], :pointer
  attach_function :kore_model_new,      [:int, :int, :int],           :pointer
  attach_function :kore_model_free,     [:pointer],                   :void
  attach_function :kore_model_fit,      [:pointer, :pointer, :uint64, :uint64, :pointer], :int
  attach_function :kore_model_predict,  [:pointer, :pointer, :uint64, :uint64, :pointer], :int

  def self.check_error(rc)
    raise "KORE error: #{kore_last_error}" if rc != 0
  end

  # ── Block ───────────────────────────────────────────────────────────────────
  class Block
    def initialize(ptr = nil)
      @ptr = ptr || Kore.kore_block_new
      raise "Failed to create KoreBlock" if @ptr.nil? || @ptr.null?
      ObjectSpace.define_finalizer(self, self.class.finalizer(@ptr))
    end

    def self.finalizer(ptr)
      proc { Kore.kore_block_free(ptr) }
    end

    def num_rows;  Kore.kore_block_num_rows(@ptr); end
    def num_cols;  Kore.kore_block_num_cols(@ptr); end

    def add_f64(name, data)
      buf = FFI::MemoryPointer.new(:double, data.size)
      buf.write_array_of_double(data)
      Kore.check_error(Kore.kore_block_add_f64(@ptr, name, buf, data.size))
      self
    end

    def add_i64(name, data)
      buf = FFI::MemoryPointer.new(:int64, data.size)
      buf.write_array_of_int64(data)
      Kore.check_error(Kore.kore_block_add_i64(@ptr, name, buf, data.size))
      self
    end

    def get_f64(col)
      n   = num_rows
      buf = FFI::MemoryPointer.new(:double, n)
      rc  = Kore.kore_block_get_f64(@ptr, col, buf, n)
      raise "KORE error: #{Kore.kore_last_error}" if rc < 0
      buf.read_array_of_double(rc)
    end

    def hash_join(right, lk, rk, how = 0)
      ptr = Kore.kore_hash_join(@ptr, right.instance_variable_get(:@ptr), lk, rk, how)
      raise "join failed: #{Kore.kore_last_error}" if ptr.nil? || ptr.null?
      Block.new(ptr)
    end

    def to_s; "KoreBlock(rows=#{num_rows}, cols=#{num_cols})"; end
  end

  # ── Model ───────────────────────────────────────────────────────────────────
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
      @ptr = Kore.kore_model_new(type, param1, param2)
      raise "model creation failed: #{Kore.kore_last_error}" if @ptr.nil? || @ptr.null?
      ObjectSpace.define_finalizer(self, self.class.finalizer(@ptr))
    end

    def self.finalizer(ptr)
      proc { Kore.kore_model_free(ptr) }
    end

    # x_flat: flat Array<Float>, row-major
    def fit(x_flat, n_rows, n_cols, y)
      xbuf = FFI::MemoryPointer.new(:double, x_flat.size).tap { |b| b.write_array_of_double(x_flat) }
      ybuf = FFI::MemoryPointer.new(:double, y.size).tap { |b| b.write_array_of_double(y) }
      Kore.check_error(Kore.kore_model_fit(@ptr, xbuf, n_rows, n_cols, ybuf))
      self
    end

    def predict(x_flat, n_rows, n_cols)
      xbuf = FFI::MemoryPointer.new(:double, x_flat.size).tap { |b| b.write_array_of_double(x_flat) }
      obuf = FFI::MemoryPointer.new(:double, n_rows)
      Kore.check_error(Kore.kore_model_predict(@ptr, xbuf, n_rows, n_cols, obuf))
      obuf.read_array_of_double(n_rows)
    end
  end
end
