path = 'bench_final_truth.py'
with open(path, 'r', encoding='utf-8') as f:
    code = f.read()

code = code.replace(
    "'Column pruning':         False,  # not in .hkore yet",
    "'Column pruning':         True,   # read_hybrid(columns=[...])"
)
code = code.replace(
    "'Nested types':           False,  # structs/lists not in .hkore",
    "'Nested types (lists)':   True,   # LIST_I64, LIST_F64, LIST_STR"
)
code = code.replace(
    'test("KORE column pruning: NOT YET (honest)", True, "TODO: implement selective column read")',
    """k_1col = bench(lambda: kore.read_hybrid(f'{P}/num.hkore', columns=['price']))
    print(f"  KORE 1-col read:   {k_1col:.1f}ms  savings: {(1-k_1col/k_full)*100:.0f}%")
    test("KORE column pruning works", k_1col < k_full, f"full={k_full:.1f}ms 1col={k_1col:.1f}ms")
    test("KORE pruning gives speedup", k_1col < k_full * 0.8)"""
)

with open(path, 'w', encoding='utf-8') as f:
    f.write(code)
print('Updated bench_final_truth.py')
