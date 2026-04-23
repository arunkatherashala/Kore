import csv
ages=[]
with open('sample_small.csv','r',newline='') as f:
    r=csv.reader(f)
    next(r)
    for i,row in enumerate(r):
        ages.append(int(row[1]))
        if i>=19: break
print(ages)
