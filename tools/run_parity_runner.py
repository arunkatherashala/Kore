"""
Run a parity check:
1. Ensure sample CSV exists (generate if missing)
2. Run Rust `kore_bench` to produce a KORE file
3. Extract the `age` column from CSV, compute deltas
4. Write a Killer parity test that runs `delta_encode` and compares to expected
5. Run `killer-native.exe` on the generated parity test and report PASS/FAIL

Adjust paths below if your build artifacts are elsewhere.
"""
import subprocess
import csv
from pathlib import Path
import sys

TOOLS_DIR = Path(__file__).parent
ROOT = TOOLS_DIR.parent
CSV = TOOLS_DIR / 'sample_small.csv'
KORE_OUT = TOOLS_DIR / 'sample_small_kore_builtin.kore'
PARITY_TEST = ROOT / 'kore_fileformat_killer' / 'parity_auto.killer'

# Edit these if your artifacts live elsewhere
KORE_BENCH = Path(r"A:\My workspace\killer_A15\killer\SOURCE\src\v2-rust\killer\target\release\kore_bench.exe")
KILLER_NATIVE = Path(r"A:\My workspace\killer_A15\killer\SOURCE\src\v2-rust\killer\target\release\killer-native.exe")

PY = sys.executable


def ensure_csv():
    if CSV.exists():
        print(f"Found CSV: {CSV}")
        return
    print("CSV missing; generating 1MB sample_small.csv")
    gen = TOOLS_DIR / 'generate_csv.py'
    if not gen.exists():
        raise FileNotFoundError("generate_csv.py not found in tools/")
    subprocess.check_call([PY, str(gen), str(CSV.name), '1'], cwd=str(TOOLS_DIR))


def run_kore_bench():
    if not KORE_BENCH.exists():
        raise FileNotFoundError(f"kore_bench.exe not found at {KORE_BENCH}")
    print('Running kore_bench...')
    subprocess.check_call([str(KORE_BENCH), str(CSV), str(KORE_OUT)])
    print('kore_bench finished; KORE at', KORE_OUT)


def compute_ages_and_deltas(max_rows=None):
    ages = []
    with open(CSV, 'r', newline='') as f:
        r = csv.reader(f)
        headers = next(r)
        for i, row in enumerate(r):
            try:
                ages.append(int(row[1]))
            except Exception:
                # skip malformed
                continue
            if max_rows and i+1 >= max_rows:
                break
    if not ages:
        return [], []
    deltas = [ages[0]]
    for i in range(1, len(ages)):
        deltas.append(ages[i] - ages[i-1])
    return ages, deltas


def write_killer_parity(ages, deltas):
    lines = []
    lines.append('// Auto parity test')
    lines.append('print("parity_auto: compare Killer delta to Python reference\\n")')
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
    # Run with working directory set to the parity test folder and pass a relative filename
    proc = subprocess.run([str(KILLER_NATIVE), str(PARITY_TEST.name)], capture_output=True, text=True, cwd=str(PARITY_TEST.parent))
    print('--- killer-native stdout ---')
    print(proc.stdout)
    print('--- killer-native stderr ---')
    print(proc.stderr)
    return proc.returncode, proc.stdout


if __name__ == '__main__':
    ensure_csv()
    # run kore_bench to produce canonical KORE (not strictly needed for the small parity check,
    # but we include it to match the full workflow)
    try:
        run_kore_bench()
    except Exception as e:
        print('kore_bench failed or missing:', e)
        print('Proceeding with parity test using CSV-extracted ages')
    ages, deltas = compute_ages_and_deltas(max_rows=100)
    if not ages:
        print('No ages extracted; aborting')
        sys.exit(2)
    write_killer_parity(ages, deltas)
    code, out = run_killer_parity()
    if 'PARITY PASS' in out:
        print('Runner result: PARITY PASS')
        # update TODO externally if desired
        sys.exit(0)
    else:
        print('Runner result: PARITY FAIL')
        sys.exit(1)
