import zipfile
z = zipfile.ZipFile('wheels/kore_fileformat-1.1.4-cp312-cp312-win_amd64.whl')
metadata = z.read('kore_fileformat-1.1.4.dist-info/METADATA').decode()
has_license = False
for line in metadata.split('\n'):
    if 'license' in line.lower():
        print(f"Found: {line}")
        has_license = True

if not has_license:
    print("✅ No license field found in local wheel!")
else:
    print("\n❌ License field still present!")
    print("\nFull header (first 50 lines):")
    for i, line in enumerate(metadata.split('\n')[:50]):
        print(f"{i+1}: {line}")
