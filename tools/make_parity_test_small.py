import csv

ages=[]
with open('sample_small.csv','r',newline='') as f:
    r=csv.reader(f)
    next(r)
    for i,row in enumerate(r):
        ages.append(int(row[1]))
        if i>=99: break
if len(ages)>0:
    deltas=[ages[0]]
    for i in range(1,len(ages)):
        deltas.append(ages[i]-ages[i-1])
else:
    deltas=[]
content = []
content.append('// Parity delta test (first 100 rows)')
content.append('print("parity_delta_test: compare Killer delta to Python reference\\n")')
content.append('let ages = ' + repr(ages))
content.append('let expected = ' + repr(deltas))
content.append('fn delta_encode(xs)')
content.append('{')
content.append('    if len(xs) == 0 { return [] }')
content.append('    let prev = xs[0]')
content.append('    let out = [prev]')
content.append('    let i = 1')
content.append('    while i < len(xs)')
content.append('    {')
content.append('        let v = xs[i]')
content.append('        let d = v - prev')
content.append('        out = concat(out, [d])')
content.append('        prev = v')
content.append('        i = i + 1')
content.append('    }')
content.append('    return out')
content.append('}')
content.append('let deltas = delta_encode(ages)')
content.append('print(K"ages: {ages}")'.replace('{ages}', str(ages)))
content.append('print(K"deltas: {deltas}")'.replace('{deltas}', str(deltas)))
content.append('print(K"expected: {expected}")'.replace('{expected}', str(deltas)))
content.append('let ok = (str(deltas) == str(expected))')
content.append('if ok { print("PARITY PASS") } else { print("PARITY FAIL") }')
with open('../kore_fileformat_killer/parity_delta_test.killer','w',encoding='utf-8') as f:
    f.write('\n'.join(content)+ '\n')
print('Wrote small parity test to kore_fileformat_killer/parity_delta_test.killer')