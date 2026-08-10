import sys; sys.path.insert(0,'kore-python')
import kore_fileformat as kore
for fn in ['can_skip_file','filter_eq','filter_range','write_partitioned',
           'parallel_read','read_file_mmap','write_file_append','write_file_stream']:
    exists = hasattr(kore, fn)
    print(f"  {fn:<25} {'EXISTS' if exists else 'MISSING'}")
