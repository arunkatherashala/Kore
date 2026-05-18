import zipfile
import sys

wheel = r'github-artifacts\wheels-windows-latest\kore_fileformat-1.1.4-cp312-cp312-win_amd64.whl'
z = zipfile.ZipFile(wheel)
metadata = z.read('kore_fileformat-1.1.4.dist-info/METADATA').decode()

print("Checking for license-file field...")
lines = metadata.split('\n')
for i, line in enumerate(lines[:100]):
    if 'license' in line.lower() or 'License' in line:
        print(f"Line {i+1}: {line}")

if not any('license-file' in l.lower() for l in lines):
    print("\n✅ No license-file field found!")
else:
    print("\n❌ license-file field found!")
