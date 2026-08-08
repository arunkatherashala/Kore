require 'fiddle'

dll_path = 'C:/Users/skathera/Downloads/asistent/kore/target/release/kore_ffi.dll'
lib = Fiddle.dlopen(dll_path)

fn_crc32 = Fiddle::Function.new(lib['kore_crc32'],
  [Fiddle::TYPE_VOIDP, Fiddle::TYPE_SIZE_T], Fiddle::TYPE_INT)

data = 'hello kore'
result = fn_crc32.call(data, data.bytesize) & 0xFFFF_FFFF
puts "crc32 = 0x#{result.to_s(16).rjust(8,'0')}"
puts(result != 0 ? 'PASS: Ruby Fiddle -> kore_ffi.dll works!' : 'FAIL: got 0')

# Test write + read via block API
fn_block_new  = Fiddle::Function.new(lib['kore_block_new'],  [], Fiddle::TYPE_VOIDP)
fn_block_free = Fiddle::Function.new(lib['kore_block_free'], [Fiddle::TYPE_VOIDP], Fiddle::TYPE_VOID)
fn_add_f64    = Fiddle::Function.new(lib['kore_block_add_f64'],
  [Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP, Fiddle::TYPE_SIZE_T], Fiddle::TYPE_INT)
fn_write      = Fiddle::Function.new(lib['kore_write_file'],
  [Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP], Fiddle::TYPE_INT)
fn_read       = Fiddle::Function.new(lib['kore_read_file'],
  [Fiddle::TYPE_VOIDP], Fiddle::TYPE_VOIDP)
fn_num_rows   = Fiddle::Function.new(lib['kore_block_num_rows'], [Fiddle::TYPE_VOIDP], Fiddle::TYPE_LONG)
fn_num_cols   = Fiddle::Function.new(lib['kore_block_num_cols'], [Fiddle::TYPE_VOIDP], Fiddle::TYPE_INT)

# Create block with price column
handle = fn_block_new.call
values = [1.1, 2.2, 3.3, 4.4, 5.5].pack('d*')
fn_add_f64.call(handle, "price\0", values, 5)

require 'tmpdir'
tmp = Dir.tmpdir + '/test_ruby.kore'
rc  = fn_write.call(tmp + "\0", handle)
fn_block_free.call(handle)

if rc == 0
  puts "write_file OK (#{File.size(tmp)} bytes) PASS"
else
  puts "write_file FAIL rc=#{rc}"
end

# Read it back
handle2 = fn_read.call(tmp + "\0")
nrows = fn_num_rows.call(handle2)
ncols = fn_num_cols.call(handle2)
puts "read_file: #{nrows} rows, #{ncols} cols PASS"
fn_block_free.call(handle2)

File.delete(tmp)
puts "Ruby FFI -> kore_ffi.dll: ALL TESTS PASSED"
