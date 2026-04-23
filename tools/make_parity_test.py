import csv

ages=[]
with open('sample_small.csv','r',newline='') as f:
    r=csv.reader(f)
    next(r)
    for row in r:
        ages.append(int(row[1]))
# compute deltas
if len(ages)>0:
    deltas=[ages[0]]
    for i in range(1,len(ages)):
        deltas.append(ages[i]-ages[i-1])
else:
    deltas=[]
# write Killer parity test file
kiler_lines = []
kiler_lines.append('// Parity delta test generated from sample_small.csv')
kiler_lines.append('print("parity_delta_test: compare Killer delta to Python reference\\n")')
kiler_lines.append('let ages = ' + repr(ages))
kiler_lines.append('let expected = ' + repr(deltas))
kiler_lines.append('fn delta_encode(xs)')
kiler_lines.append('{')
kiler_lines.append('    if len(xs) == 0 { return [] }')
kiler_lines.append('    let prev = xs[0]')
kiler_lines.append('    let out = [prev]')
kiler_lines.append('    let i = 1')
kiler_lines.append('    while i < len(xs)')
kiler_lines.append('    {')
kiler_lines.append('        let v = xs[i]')
kiler_lines.append('        let d = v - prev')
kiler_lines.append('        out = concat(out, [d])')
kiler_lines.append('        prev = v')
kiler_lines.append('        i = i + 1')
kiler_lines.append('    }')
kiler_lines.append('    return out')
kiler_lines.append('}')
kiler_lines.append('let deltas = delta_encode(ages)')
kiler_lines.append('print(K"ages: {ages}")'.replace('{ages}', str(ages)))
kiler_lines.append('print(K"deltas: {deltas}")'.replace('{deltas}', str(deltas)))
kiler_lines.append('print(K"expected: {expected}")'.replace('{expected}', str(deltas)))
kiler_lines.append('let ok = (str(deltas) == str(expected))')
kiler_lines.append('if ok { print("PARITY PASS") } else { print("PARITY FAIL") }')
with open('../kore_fileformat_killer/parity_delta_test.killer','w',encoding='utf-8') as f:
    f.write('\n'.join(kiler_lines) + '\n')
print('Wrote parity test to kore_fileformat_killer/parity_delta_test.killer')