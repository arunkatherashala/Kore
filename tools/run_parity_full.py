"""
Run a full parity check on the large sample CSV (`sample_10mb.csv`).

This replicates `run_parity_runner.py` but targets the 10MB file and writes
its parity test to `kore_fileformat_killer/parity_full_10mb.killer`.
"""
import subprocess
import csv
from pathlib import Path
import sys

TOOLS_DIR = Path(__file__).parent
ROOT = TOOLS_DIR.parent
CSV = TOOLS_DIR / 'sample_10mb.csv'
PARITY_TEST = ROOT / 'kore_fileformat_killer' / 'parity_full_10mb.killer'
KORE_OUT = TOOLS_DIR / 'sample_10mb_kore_builtin_full.kore'

# Edit these paths if your artifacts are elsewhere
KORE_BENCH = Path(r"A:\My workspace\killer_A15\killer\SOURCE\src\v2-rust\killer\target\release\kore_bench.exe")
KILLER_NATIVE = Path(r"A:\My workspace\killer_A15\killer\SOURCE\src\v2-rust\killer\target\release\killer-native.exe")
PY = sys.executable


def run_kore_bench():
    if not KORE_BENCH.exists():
        raise FileNotFoundError(f"kore_bench.exe not found at {KORE_BENCH}")
    print('Running kore_bench...')
    subprocess.check_call([str(KORE_BENCH), str(CSV), str(KORE_OUT)])
    print('kore_bench finished; KORE at', KORE_OUT)


def compute_ages_and_deltas():
    ages = []
    with open(CSV, 'r', newline='') as f:
        r = csv.reader(f)
        headers = next(r)
        for row in r:
            try:
                ages.append(int(row[1]))
            except Exception:
                continue
    if not ages:
        return [], []
    deltas = [ages[0]]
    for i in range(1, len(ages)):
        deltas.append(ages[i] - ages[i-1])
    return ages, deltas


def write_killer_parity(ages, deltas):
    lines = []
    lines.append('// Auto parity test (full 10MB)')
    lines.append('print("parity_full_10mb: compare Killer delta to Python reference\\n")')
    lines.append('let ages = ' + repr(ages))
    lines.append('let expected = ' + repr(deltas))
    lines.append('fn delta_encode(xs)')
    lines.append('{')
    lines.append('    if len(xs) == 0 {')
    lines.append('        return []')
    lines.append('    }')
    lines.append('    let prev = xs[0]')
    lines.append('    let out = [prev]')
    lines.append('    let i = 1')
    lines.append('    while i < len(xs)')
    lines.append('    {')
    lines.append('        let v = xs[i]')
    lines.append('        let d = v - prev')
    lines.append('        out = concat(out, [d])')
    lines.append('        prev = v')
    lines.append('        i = i + 1')
    lines.append('    }')
    lines.append('    return out')
    lines.append('}')
    lines.append('')
    lines.append('let deltas = delta_encode(ages)')
    lines.append('print(K"ages: {ages}")'.replace('{ages}', str(ages)))
    lines.append('print(K"deltas: {deltas}")'.replace('{deltas}', str(deltas)))
    lines.append('print(K"expected: {expected}")'.replace('{expected}', str(deltas)))
    lines.append('let ok = (str(deltas) == str(expected))')
    lines.append('if ok {')
    lines.append('    print("PARITY PASS")')
    lines.append('} else {')
    lines.append('    print("PARITY FAIL")')
    lines.append('}')
    PARITY_TEST.parent.mkdir(parents=True, exist_ok=True)
    with open(PARITY_TEST, 'w', encoding='utf-8') as f:
        f.write('\n'.join(lines) + '\n')
    print('Wrote parity test to', PARITY_TEST)


def run_killer_parity():
    if not KILLER_NATIVE.exists():
        raise FileNotFoundError(f"killer-native.exe not found at {KILLER_NATIVE}")
    print('Running killer-native on parity test...')
    proc = subprocess.run([str(KILLER_NATIVE), str(PARITY_TEST.name)], capture_output=True, text=True, cwd=str(PARITY_TEST.parent))
    print('--- killer-native stdout ---')
    print(proc.stdout)
    print('--- killer-native stderr ---')
    print(proc.stderr)
    return proc.returncode, proc.stdout


if __name__ == '__main__':
    if not CSV.exists():
        print('CSV not found:', CSV)
        sys.exit(2)
    try:
        run_kore_bench()
    except Exception as e:
        print('kore_bench failed or missing:', e)
        print('Proceeding with parity test using CSV-extracted ages')
    ages, deltas = compute_ages_and_deltas()
    if not ages:
        print('No ages extracted; aborting')
        sys.exit(2)
    write_killer_parity(ages, deltas)
    code, out = run_killer_parity()
    if 'PARITY PASS' in out:
        print('Runner result: PARITY PASS')
        sys.exit(0)
    else:
        print('Runner result: PARITY FAIL')
        sys.exit(1)
