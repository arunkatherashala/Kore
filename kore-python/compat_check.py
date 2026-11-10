"""Release compatibility checks for the Python module (kore_fileformat.py, the code published to PyPI).

Two directions:

  fixtures   OLD FILES, NEW CODE. Opens every committed .kore fixture pinned in
             kore-store/tests/fixtures/old_files.digest (written by older versions) and checks the
             decoded data against the pinned digest (the same digest kore-store/tests/old_files.rs
             checks in Rust). Version logs are checked version by version.
             Not readable from Python: format-4 files whose text column references a shared
             dictionary (.kdic) -- the Python module has no API to pass dictionaries. For those the
             check is that reading is refused with a clear error that names the dictionary.

  write DIR  NEW FILES, OLD CODE (step 1, run with the CURRENT module). Writes files in formats an
             older release supports (native layout, legacy layout, row groups, version log) plus
             files an older release must refuse or read exactly (rule mode v6) or must refuse
             (container versions from the future, an unknown text-header layout, a truncated file),
             and DIR/manifest.json with the expected digest of each.
  read DIR   NEW FILES, OLD CODE (step 2, run with the OLD module, e.g. pip install
             kore-fileformat==1.8.21). Every "read" file must decode to the expected digest; every
             "refuse" file must raise a Python exception with a non-empty message (each attempt runs
             in its own subprocess, so a crash or hang is detected and reported as a failure, not
             mistaken for a refusal); "refuse-or-exact" files must do one of the two, never return
             different data.
  no-native DIR
             The PyPI package as installed contains only kore_fileformat.py; without the kore-ffi
             native library every read must fail with the "Could not load kore-ffi library" error.
  selftest   write + read into a temporary directory with the same module.

Module selection: by default `import kore_fileformat` resolves to the installed package. With
--local the in-repo kore-python/kore_fileformat.py is imported instead (needs target/release/kore_ffi
built: cargo build --release -p kore-ffi). Run with python -I so neither the current directory nor
PYTHON* variables can change which module is imported.

Local examples (from the repo root):
  python -I kore-python/compat_check.py fixtures --local
  python -I kore-python/compat_check.py selftest --local
CI (old reader):
  python -I kore-python/compat_check.py write "$DIR" --local
  "$VENV/bin/python" -I kore-python/compat_check.py read "$DIR" --expect-version 1.8.21

Exit status 0 when every check passes, 1 otherwise. Python 3.9+ (stdlib only).
"""
import argparse
import importlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
ORACLE = REPO / 'kore-store' / 'tests' / 'fixtures' / 'old_files.digest'
PASSWORD = 'kore-conformance'
FNV_OFFSET, FNV_PRIME, MASK = 0xcbf29ce484222325, 0x100000001b3, 0xFFFFFFFFFFFFFFFF
CANON_NAN = 0x7FF8_0000_0000_0001
OFFSET_LINE = 24  # "KORE2 offset=0000000000\n"
CHILD_TIMEOUT = 120


# ---------------------------------------------------------------------------------------------
# module loading and digest
# ---------------------------------------------------------------------------------------------

def load_module(local):
    if local:
        sys.path.insert(0, str(HERE))
    mod = importlib.import_module('kore_fileformat')
    if local and Path(mod.__file__).resolve().parent != HERE:
        raise SystemExit(f"--local imported {mod.__file__}, not the in-repo module")
    return mod


def _fnv(h, data):
    for b in data:
        h = ((h ^ b) * FNV_PRIME) & MASK
    return h


def digest(block):
    """Same text as digest() in kore-store/tests/old_files.rs: 'rows N' + one line per column."""
    out = [f"rows {block.num_rows}"]
    for col in block.columns:
        name = col.dtype.name if hasattr(col.dtype, 'name') else str(col.dtype)
        ty = {'I64': 'i64', 'F64': 'f64', 'BOOL': 'bool'}.get(name, 'str')
        h, nulls = FNV_OFFSET, 0
        for v in col.data:
            if v is None:
                nulls += 1
                h = _fnv(h, b'\x00')
                continue
            h = _fnv(h, b'\x01')
            if ty == 'i64':
                h = _fnv(h, struct.pack('<q', v))
            elif ty == 'f64':
                bits = CANON_NAN if v != v else struct.unpack('<Q', struct.pack('<d', v))[0]
                h = _fnv(h, struct.pack('<Q', bits))
            elif ty == 'bool':
                h = _fnv(h, bytes([int(v)]))
            else:
                raw = v.encode('utf-8')
                h = _fnv(h, struct.pack('<I', len(raw)) + raw)
        out.append(f"{col.name} {ty} {nulls} {h:016x}")
    return '\n'.join(out) + '\n'


def parse_oracle(text):
    """{path: {'flags': set, 'latest': str, 'versions': {ts: str}}}"""
    entries, cur, section = {}, None, None
    for line in text.replace('\r\n', '\n').split('\n'):
        if not line or line.startswith('#'):
            continue
        if line.startswith('== '):
            parts = line[3:].split(' ')
            cur = {'flags': set(parts[1:]), 'latest': '', 'versions': {}}
            entries[parts[0]] = cur
            section = 'latest'
        elif line.startswith('-- version '):
            section = int(line[len('-- version '):])
            cur['versions'][section] = ''
        elif section == 'latest':
            cur['latest'] += line + '\n'
        else:
            cur['versions'][section] += line + '\n'
    return entries


# ---------------------------------------------------------------------------------------------
# direction 1: old files, new code
# ---------------------------------------------------------------------------------------------

def check_fixtures(kore, log=print):
    """Returns a list of problems (empty = pass)."""
    problems = []
    entries = parse_oracle(ORACLE.read_text(encoding='utf-8'))
    if len(entries) < 30:
        problems.append(f"only {len(entries)} pinned fixtures in {ORACLE}")
    readable, refused = 0, []
    for rel, e in sorted(entries.items()):
        path = REPO / rel
        if not path.is_file():
            problems.append(f"{rel}: pinned but missing")
            continue
        try:
            if 'encrypted' in e['flags']:
                plain = kore.decrypt_aes256(PASSWORD, path.read_bytes())
                got = digest(kore._block_from_native(plain))
            elif 'needs-dicts' in e['flags']:
                try:
                    kore.read_file(path)
                except Exception as exc:  # expected: no dictionary API in Python
                    msg = str(exc)
                    if 'dictionary' not in msg:
                        problems.append(f"{rel}: refused, but the error does not name the dictionary: {msg}")
                    refused.append(rel)
                    continue
                problems.append(f"{rel}: needs a shared dictionary but read without one")
                continue
            else:
                got = digest(kore.read_file(path))
        except Exception as exc:
            problems.append(f"{rel}: cannot be opened by this module: {type(exc).__name__}: {exc}")
            continue
        if got != e['latest']:
            problems.append(f"{rel}: decodes differently than pinned\n  got:\n{got}  pinned:\n{e['latest']}")
            continue
        for ts, want in sorted(e['versions'].items()):
            try:
                got_v = digest(kore.read_at_version(path.read_bytes(), ts))
            except Exception as exc:
                problems.append(f"{rel} version {ts}: {type(exc).__name__}: {exc}")
                continue
            if got_v != want:
                problems.append(f"{rel} version {ts}: decodes differently than pinned")
        readable += 1
    # the Python-only container (text header + legacy layout) committed for the Python tests
    legacy = HERE / 'fixtures' / 'legacy_v3.kore'
    try:
        b = kore.read_file(legacy)
        if b.num_rows != 5 or [c.name for c in b.columns] != ['i', 'f', 's', 'b']:
            problems.append(f"{legacy.name}: unexpected shape {b.num_rows} rows {[c.name for c in b.columns]}")
        else:
            readable += 1
    except Exception as exc:
        problems.append(f"{legacy.name}: {type(exc).__name__}: {exc}")
    log(f"fixtures: {readable} read and matched, {len(refused)} refused as expected "
        f"(need a shared .kdic dictionary, no Python API): {', '.join(Path(r).name for r in refused) or '-'}")
    return problems


# ---------------------------------------------------------------------------------------------
# direction 2: new files, old code
# ---------------------------------------------------------------------------------------------

def sample_block(kore, n=600):
    b = kore.DataBlock()
    b.add_column('i', kore.DataType.I64,
                 [None if k % 10 == 3 else (-(2 ** 63) if k == 0 else k * 7919 - 1_000_000) for k in range(n)])
    b.add_column('f', kore.DataType.F64,
                 [None if k % 13 == 2 else (-0.0 if k == 7 else (float('nan') if k % 97 == 5 else k / 8.0))
                  for k in range(n)])
    b.add_column('s', kore.DataType.STR,
                 [None if k % 11 == 0 else ('' if k % 17 == 1 else f"café\U0001F600-{k % 37}") for k in range(n)])
    b.add_column('b', kore.DataType.BOOL, [None if k % 7 == 0 else k % 3 == 0 for k in range(n)])
    return b


def _binary_start(data):
    if data[:5] == b'KORE2' and len(data) >= OFFSET_LINE:
        return int(data[13:23])
    return 0


def _with_container_version(data, version):
    """Same file with the u16 container version (bytes 4..6 after the 'KORE' magic) replaced."""
    start = _binary_start(data)
    if data[start:start + 4] != b'KORE':
        raise ValueError("no KORE container found")
    return data[:start + 4] + struct.pack('<H', version) + data[start + 6:]


def write_cases(kore, out_dir, log=print):
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    block = sample_block(kore)
    want = digest(block)
    cases = []

    def read_case(name, expected):
        cases.append({'file': name, 'expect': 'read', 'digest': expected})

    kore.write_file(out / 'native.kore', block)
    read_case('native.kore', want)

    old = os.environ.get('KORE_LEGACY_LAYOUT')
    os.environ['KORE_LEGACY_LAYOUT'] = '1'
    try:
        kore.write_file(out / 'legacy_layout.kore', block)
    finally:
        if old is None:
            os.environ.pop('KORE_LEGACY_LAYOUT', None)
        else:
            os.environ['KORE_LEGACY_LAYOUT'] = old
    read_case('legacy_layout.kore', digest(kore.read_file(out / 'legacy_layout.kore')))

    kore.write_file(out / 'row_groups.kore', block, row_group_rows=100)
    read_case('row_groups.kore', want)

    small = kore.DataBlock()
    small.add_column('only', kore.DataType.I64, [1, 2])
    kore.append_version(out / 'versions.kore', small, timestamp=100)
    kore.append_version(out / 'versions.kore', block, timestamp=200)
    # a Python-written log holds text-header entries, so it is read through read_at_version
    # (read_file on such a log is not supported by this module, old or new)
    cases.append({'file': 'versions.kore', 'expect': 'read-version', 'timestamp': 200, 'digest': want})
    cases.append({'file': 'versions.kore', 'expect': 'read-version', 'timestamp': 100, 'digest': digest(small)})

    # the current module must itself read back what it wrote (otherwise the expectations are wrong)
    for c in cases:
        p = out / c['file']
        got = digest(kore.read_at_version(p.read_bytes(), c['timestamp'])) if c['expect'] == 'read-version' \
            else digest(kore.read_file(p))
        if got != c['digest']:
            raise SystemExit(f"current module does not read back its own {c['file']}")
    if digest(kore.read_file(out / 'legacy_layout.kore')) != want:
        log("note: the legacy layout (KORE_LEGACY_LAYOUT=1) does not round-trip the whole sample "
            "(it stores None strings as '' and i64::MIN reads back as 0); the old reader is checked "
            "against what the current reader returns for that file")

    # rule mode (container version 6): an older reader must refuse it or decode exactly this data
    entries = parse_oracle(ORACLE.read_text(encoding='utf-8'))
    shutil.copyfile(REPO / 'kore-store' / 'tests' / 'fixtures' / 'rules_v6.kore', out / 'rules_v6.kore')
    cases.append({'file': 'rules_v6.kore', 'expect': 'refuse-or-exact',
                  'digest': entries['kore-store/tests/fixtures/rules_v6.kore']['latest']})

    # newer than anything supported: must be refused
    native = (out / 'native.kore').read_bytes()
    raw = native[_binary_start(native):]
    if b'KORE' not in raw:
        raise SystemExit("unexpected native.kore layout")
    raw = raw[raw.find(b'KORE'):]  # the Rust container after the text header and its marker
    for v in (7, 99, 0xFFFF):
        name = f'future_container_v{v}.kore'
        (out / name).write_bytes(_with_container_version(raw, v))
        cases.append({'file': name, 'expect': 'refuse', 'must_mention': 'version'})
    rules = (out / 'rules_v6.kore').read_bytes()
    (out / 'future_rules_v7.kore').write_bytes(_with_container_version(rules, 7))
    cases.append({'file': 'future_rules_v7.kore', 'expect': 'refuse', 'must_mention': 'version'})
    marker = b'# Layout: native-1\n'
    if marker not in native:
        raise SystemExit("native.kore has no '# Layout: native-1' line")
    (out / 'future_layout.kore').write_bytes(native.replace(marker, b'# Layout: native-9\n', 1))
    cases.append({'file': 'future_layout.kore', 'expect': 'refuse', 'must_mention': 'layout'})
    (out / 'truncated_rules_v6.kore').write_bytes(rules[:len(rules) // 2])
    cases.append({'file': 'truncated_rules_v6.kore', 'expect': 'refuse'})

    (out / 'manifest.json').write_text(json.dumps({'cases': cases}, indent=1), encoding='utf-8')
    log(f"wrote {len(cases)} cases to {out}")
    return cases


def _child(args, local):
    """Run `python -I compat_check.py _one ...` and return its JSON result (or a crash description)."""
    cmd = [sys.executable, '-I', str(Path(__file__).resolve()), '_one'] + args + (['--local'] if local else [])
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=CHILD_TIMEOUT)
    except subprocess.TimeoutExpired:
        return {'status': 'crash', 'detail': f'timed out after {CHILD_TIMEOUT}s'}
    lines = [l for l in p.stdout.splitlines() if l.startswith('{')]
    if p.returncode != 0 or not lines:
        return {'status': 'crash', 'detail': f'exit code {p.returncode}; stderr: {p.stderr.strip()[-800:]}'}
    return json.loads(lines[-1])


def one(path, timestamp, local):
    """Child process: read one file, print one JSON line. Python exceptions are a result, not a crash."""
    kore = load_module(local)
    try:
        if timestamp is None:
            b = kore.read_file(path)
        else:
            b = kore.read_at_version(Path(path).read_bytes(), timestamp)
        print(json.dumps({'status': 'ok', 'digest': digest(b)}))
    except Exception as exc:
        print(json.dumps({'status': 'error', 'type': type(exc).__name__, 'message': str(exc)}))


def read_cases(in_dir, local, log=print):
    d = Path(in_dir)
    cases = json.loads((d / 'manifest.json').read_text(encoding='utf-8'))['cases']
    problems = []
    for c in cases:
        ts = c.get('timestamp') if c['expect'] == 'read-version' else None
        r = _child([str(d / c['file'])] + (['--timestamp', str(ts)] if ts is not None else []), local)
        label = c['file'] + (f" @version {ts}" if ts is not None else '')
        if r['status'] == 'crash':
            problems.append(f"{label}: CRASH ({r['detail']})")
            continue
        if c['expect'] in ('read', 'read-version'):
            if r['status'] != 'ok':
                problems.append(f"{label}: should read, got {r['type']}: {r['message']}")
            elif r['digest'] != c['digest']:
                problems.append(f"{label}: WRONG DATA\n  got:\n{r['digest']}  expected:\n{c['digest']}")
            else:
                log(f"ok   {label}: read, data matches")
        elif c['expect'] == 'refuse':
            if r['status'] == 'ok':
                problems.append(f"{label}: should be refused but was read ({r['digest'].splitlines()[0]})")
            elif not r['message'].strip():
                problems.append(f"{label}: refused with an empty message ({r['type']})")
            elif c.get('must_mention') and c['must_mention'] not in r['message'].lower():
                problems.append(f"{label}: refused, but the message does not mention "
                                f"'{c['must_mention']}': {r['type']}: {r['message']}")
            else:
                log(f"ok   {label}: refused cleanly ({r['type']}: {r['message']})")
        else:  # refuse-or-exact
            if r['status'] == 'ok':
                if r['digest'] != c['digest']:
                    problems.append(f"{label}: read but WRONG DATA\n  got:\n{r['digest']}  expected:\n{c['digest']}")
                else:
                    log(f"ok   {label}: this reader supports it, data matches")
            elif not r['message'].strip():
                problems.append(f"{label}: refused with an empty message ({r['type']})")
            else:
                log(f"ok   {label}: refused cleanly ({r['type']}: {r['message']})")
    return problems


def no_native(in_dir, kore, log=print):
    try:
        kore.read_file(Path(in_dir) / 'native.kore')
    except RuntimeError as exc:
        if 'kore-ffi' in str(exc):
            log("ok   without the native library the package refuses with: " + str(exc).splitlines()[0])
            return []
        return [f"RuntimeError without a kore-ffi hint: {exc}"]
    except Exception as exc:
        return [f"expected RuntimeError('Could not load kore-ffi library'), got {type(exc).__name__}: {exc}"]
    return ["read succeeded without the native library on the search path (test setup is wrong)"]


def describe_module(kore, expect_version, local):
    problems = []
    where = Path(kore.__file__).resolve()
    try:
        from importlib import metadata
        dist = metadata.version('kore-fileformat')
    except Exception:
        dist = None
    print(f"module: {where} (installed distribution kore-fileformat: {dist or 'none'})")
    if expect_version:
        if local:
            problems.append("--expect-version cannot be combined with --local")
        if dist != expect_version:
            problems.append(f"expected installed kore-fileformat {expect_version}, found {dist}")
        if REPO in where.parents:
            problems.append(f"module was imported from the repository ({where}), not from the installed package")
    return problems


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('mode', choices=['fixtures', 'write', 'read', 'no-native', 'selftest', '_one'])
    ap.add_argument('path', nargs='?')
    ap.add_argument('--local', action='store_true', help='import the in-repo kore-python/kore_fileformat.py')
    ap.add_argument('--expect-version', help='require this installed kore-fileformat version (read mode)')
    ap.add_argument('--timestamp', type=int, help=argparse.SUPPRESS)
    a = ap.parse_args(argv)

    if a.mode == '_one':
        one(a.path, a.timestamp, a.local)
        return 0

    kore = load_module(a.local)
    problems = describe_module(kore, a.expect_version, a.local)
    if a.mode in ('write', 'read', 'no-native') and not a.path:
        ap.error(f"{a.mode} needs a directory")
    if a.mode == 'fixtures':
        problems += check_fixtures(kore)
    elif a.mode == 'write':
        write_cases(kore, a.path)
    elif a.mode == 'read':
        problems += read_cases(a.path, a.local)
    elif a.mode == 'no-native':
        problems += no_native(a.path, kore)
    else:  # selftest
        tmp = tempfile.mkdtemp(prefix='kore-compat-')
        try:
            write_cases(kore, tmp)
            problems += read_cases(tmp, a.local)
        finally:
            shutil.rmtree(tmp, ignore_errors=True)

    for p in problems:
        print('FAIL ' + p)
    print('PASS' if not problems else f'{len(problems)} problem(s)')
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
