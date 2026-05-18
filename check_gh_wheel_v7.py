import zipfile

wheel = r'github-artifacts-v7\wheels-windows-latest\kore_fileformat-1.1.4-cp312-cp312-win_amd64.whl'
z = zipfile.ZipFile(wheel)
metadata_content = z.read('kore_fileformat-1.1.4.dist-info/METADATA').decode()

print("Checking for license-file field...")
has_license_file = False
for i, line in enumerate(metadata_content.split('\n')[:100]):
    if 'license-file' in line.lower():
        print(f"Line {i+1}: {line}")
        has_license_file = True

if not has_license_file:
    print("✅ NO license-file field found in the header!")
else:
    print("❌ license-file field found!")

# Also print the first 30 lines to see the full metadata
print("\nFirst 30 lines of metadata:")
for i, line in enumerate(metadata_content.split('\n')[:30]):
    print(f"{i+1}: {line}")
