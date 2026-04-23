"""
Run binary parity test:
1. Run Rust kore_bench to produce binary KORE for a small test CSV
2. Run Killer parity_binary_test.killer to produce Killer hex output (file or stdout)
3. Compare hex of Rust KORE to Killer hex and report PASS/FAIL
"""
import subprocess
from pathlib import Path
import sys

TOOLS_DIR = Path(__file__).parent
ROOT = TOOLS_DIR.parent
KORE_BENCH = Path(r"A:\My workspace\killer_A15\killer\SOURCE\src\v2-rust\killer\target\release\kore_bench.exe")
KILLER_NATIVE = Path(r"A:\My workspace\killer_A15\killer\SOURCE\src\v2-rust\killer\target\release\killer-native.exe")
CSV = TOOLS_DIR / 'sample_small.csv'
RUST_OUT = TOOLS_DIR / 'sample_small_kore_builtin.kore'
KILLER_TEST = ROOT / 'kore_fileformat_killer' / 'parity_binary_test.killer'
KILLER_HEX = TOOLS_DIR / 'killer_binary_out.hex'

PY = sys.executable


def run_kore_bench():
    if not KORE_BENCH.exists():
        raise FileNotFoundError("kore_bench.exe not found")
    subprocess.check_call([str(KORE_BENCH), str(CSV), str(RUST_OUT)])


def run_killer():
    if not KILLER_NATIVE.exists():
        raise FileNotFoundError("killer-native.exe not found")
    proc = subprocess.run([str(KILLER_NATIVE), str(KILLER_TEST.name)], cwd=str(KILLER_TEST.parent), capture_output=True, text=True)
    # check for file output in the test folder
    file_path = KILLER_TEST.parent / 'killer_binary_out.hex'
    if file_path.exists():
        with open(file_path, 'r', encoding='utf-8') as f:
            return f.read().strip()
    # else parse stdout for hex-like output
    out = (proc.stdout or "") + "\n" + (proc.stderr or "")
    out = out.strip()
    # look for explicit writer result
    for line in out.splitlines():
        if 'writer result:' in line.lower():
            # split on the first colon
            parts = line.split(':', 1)
            if len(parts) > 1:
                return parts[1].strip()
    # fallback: find the longest hex-like token (>= 32 hex chars)
    import re
    tokens = re.findall(r"[0-9a-fA-F]{32,}", out)
    if tokens:
        # return the longest token
        return max(tokens, key=len)
    return None


def read_rust_hex():
    with open(RUST_OUT,'rb') as f:
        data = f.read()
    return data.hex()


if __name__ == '__main__':
    try:
        run_kore_bench()
    except Exception as e:
        print('kore_bench failed:', e)
        sys.exit(2)
    rust_hex = read_rust_hex()
    killer_hex = run_killer()
    if killer_hex is None:
        print('Killer did not produce hex output')
        sys.exit(2)
    if rust_hex == killer_hex:
        print('BINARY PARITY PASS')
        sys.exit(0)
    else:
        print('BINARY PARITY FAIL')
        # Try canonicalized fallback: parse both hex outputs according to the deterministic
        # KORE prototype format (MAGIC 'KORE', version, schema_len/schema, rows...)
        def try_parse_kore(hexstr):
            try:
                data = bytes.fromhex(hexstr)
                idx = 0
                if len(data) < 8:
                    return None
                magic = data[0:4].decode('ascii', errors='ignore')
                if magic != 'KORE':
                    return None
                idx = 4
                def read_u32():
                    nonlocal idx
                    if idx + 4 > len(data):
                        raise ValueError('truncated')
                    v = int.from_bytes(data[idx:idx+4], 'little')
                    idx += 4
                    return v
                version = read_u32()
                schema_len = read_u32()
                if idx + schema_len > len(data):
                    return None
                schema = data[idx:idx+schema_len].decode('utf-8', errors='ignore')
                idx += schema_len
                rows_count = read_u32()
                rows = []
                for _ in range(rows_count):
                    row_len = read_u32()
                    row = []
                    for _ in range(row_len):
                        cell_len = read_u32()
                        if idx + cell_len > len(data):
                            return None
                        cell = data[idx:idx+cell_len].decode('utf-8', errors='ignore')
                        idx += cell_len
                        row.append(cell)
                    rows.append(row)
                return (schema, rows)
            except Exception:
                return None

        rust_parsed = try_parse_kore(rust_hex)
        killer_parsed = try_parse_kore(killer_hex)
        if rust_parsed is not None and killer_parsed is not None:
            if rust_parsed == killer_parsed:
                print('CANONICAL PARITY PASS')
                sys.exit(0)
            else:
                print('CANONICAL PARITY FAIL: schema/rows differ')
        # If prototype parse failed (different on-disk layout), try a structural compare
        # using the runtime builtin `kore_info` on both binaries.
        try:
            killer_bin = TOOLS_DIR / 'killer_roundtrip.kore'
            with open(killer_bin, 'wb') as f:
                f.write(bytes.fromhex(killer_hex))
            # prepare a small Killer script to call kore_info on both files and print results
            info_script = TOOLS_DIR / '_collect_kore_info.killer'
            script_contents = []
            script_contents.append('let rust_path = "' + str(RUST_OUT).replace('\\','\\\\') + '"')
            script_contents.append('let killer_path = "' + str(killer_bin).replace('\\','\\\\') + '"')
            script_contents.append('let r = kore_info(rust_path)')
            script_contents.append('let k = kore_info(killer_path)')
            script_contents.append('print(K"KORE_INFO_RUST: {r}")')
            script_contents.append('print(K"KORE_INFO_KILLER: {k}")')
            info_script.write_text("\n".join(script_contents), encoding='utf-8')
            proc = subprocess.run([str(KILLER_NATIVE), info_script.name], cwd=str(TOOLS_DIR), capture_output=True, text=True)
            out = (proc.stdout or "") + "\n" + (proc.stderr or "")
            rust_info = None
            killer_info = None
            for line in out.splitlines():
                if line.startswith('KORE_INFO_RUST:'):
                    rust_info = line.split(':',1)[1].strip()
                if line.startswith('KORE_INFO_KILLER:'):
                    killer_info = line.split(':',1)[1].strip()
            if rust_info is not None and killer_info is not None:
                if rust_info == killer_info:
                    print('CANONICAL PARITY PASS (via kore_info)')
                    sys.exit(0)
                else:
                    print('CANONICAL PARITY FAIL (via kore_info): info differ')
        except Exception as e:
            print('canonical compare via kore_info failed:', e)
        # optionally write both to files for inspection
        with open(TOOLS_DIR / 'rust_out.hex','w',encoding='utf-8') as f:
            f.write(rust_hex)
        with open(TOOLS_DIR / 'killer_out.hex','w',encoding='utf-8') as f:
            f.write(killer_hex)
        print('Wrote rust_out.hex and killer_out.hex to tools/')
        sys.exit(1)
