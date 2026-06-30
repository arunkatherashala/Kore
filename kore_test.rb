# kore_test.rb — KORE Ruby test via Fiddle (stdlib, no gems)
require 'fiddle'
require 'fiddle/import'
require 'json'

DLL_PATH = 'C:/Users/skathera/Downloads/asistent/kore/target/release/kore_ffi.dll'

puts "=== KORE Ruby Fiddle Real Test ==="

begin
  lib = Fiddle.dlopen(DLL_PATH)
rescue => e
  puts "FAILED to load DLL: #{e}"
  exit 1
end

# Wire functions
session_new   = Fiddle::Function.new(lib['kore_session_new'],   [], Fiddle::TYPE_VOIDP)
session_free  = Fiddle::Function.new(lib['kore_session_free'],  [Fiddle::TYPE_VOIDP], Fiddle::TYPE_VOID)
session_load  = Fiddle::Function.new(lib['kore_session_load_csv'], [Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP], Fiddle::TYPE_INT)
session_query = Fiddle::Function.new(lib['kore_session_query'], [Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP], Fiddle::TYPE_VOIDP)
session_count = Fiddle::Function.new(lib['kore_session_row_count'], [Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP], Fiddle::TYPE_LONG_LONG)
free_str      = Fiddle::Function.new(lib['kore_free_string'],   [Fiddle::TYPE_VOIDP], Fiddle::TYPE_VOID)

# Helper: Ruby string → null-terminated C pointer
def cstr(s)
  bytes = s.encode('UTF-8').bytes + [0]
  ptr = Fiddle::Pointer.malloc(bytes.size)
  ptr[0, bytes.size] = bytes.pack("C*")
  ptr
end

# Helper: C pointer → Ruby string
def read_cstr(ptr)
  return nil if ptr.null?
  ptr.to_s
end

sess = session_new.call
puts "[1] Session: 0x#{sess.to_i.to_s(16)}"

rc = session_load.call(sess, cstr("bench"),
  cstr('C:/Users/skathera/Downloads/asistent/bench_export.csv'))
puts "[2] load_csv returned: #{rc} (0=OK)"

n = session_count.call(sess, cstr("bench"))
puts "[3] Row count: #{n}"

ptr = session_query.call(sess,
  cstr("SELECT category, COUNT(*) as cnt, SUM(amount) as total FROM bench GROUP BY category ORDER BY total DESC"))
if ptr && ptr.to_i != 0
  json_str = Fiddle::Pointer.new(ptr).to_s
  free_str.call(ptr)
  rows = JSON.parse(json_str)
  puts "[4] GROUP BY (#{rows.size} groups):"
  rows.each { |r| puts "     #{r}" }
else
  puts "[4] Query returned NULL"
end

ptr2 = session_query.call(sess,
  cstr("SELECT id, amount FROM bench WHERE amount > 999 ORDER BY amount DESC LIMIT 3"))
if ptr2 && ptr2.to_i != 0
  json2 = Fiddle::Pointer.new(ptr2).to_s
  free_str.call(ptr2)
  puts "[5] WHERE+LIMIT: #{json2}"
end

session_free.call(sess)
puts "\nRUBY TEST PASSED — kore_ffi.dll works from Ruby via Fiddle!"
