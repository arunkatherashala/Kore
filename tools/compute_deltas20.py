ages=[25,73,27,42,35,38,60,44,39,25,66,50,78,32,29,22,73,38,39,59]
deltas=[ages[0]]
for i in range(1,len(ages)):
    deltas.append(ages[i]-ages[i-1])
print(deltas)