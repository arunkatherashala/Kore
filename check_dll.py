import ctypes
import os

dll_path = r"c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\target\release\kore_fileformat.dll"
if not os.path.exists(dll_path):
    print(f"DLL not found at {dll_path}")
    exit(1)

try:
    lib = ctypes.CDLL(dll_path)
    print("Successfully loaded DLL")
    symbols = [
        "kore_session_new",
        "kore_session_free",
        "kore_session_load_csv",
        "kore_session_query"
    ]
    for sym in symbols:
        if hasattr(lib, sym):
            print(f"Found symbol: {sym}")
        else:
            print(f"Missing symbol: {sym}")
except Exception as e:
    print(f"Error loading DLL: {e}")
