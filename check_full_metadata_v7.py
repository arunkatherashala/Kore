import zipfile

wheel = r'github-artifacts-v7\wheels-windows-latest\kore_fileformat-1.1.4-cp312-cp312-win_amd64.whl'
z = zipfile.ZipFile(wheel)
metadata_content = z.read('kore_fileformat-1.1.4.dist-info/METADATA').decode()

# Check the ENTIRE file for license-file
if 'license-file' in metadata_content.lower():
    print("Found 'license-file' in metadata!")
    # Print the last 30 lines
    lines = metadata_content.split('\n')
    print(f"\nTotal lines: {len(lines)}")
    print("\nLast 30 lines:")
    for i, line in enumerate(lines[-30:]):
        print(f"Line {len(lines)-30+i+1}: {line}")
    
    # Also print any line containing license
    print("\nAll lines containing 'license':")
    for i, line in enumerate(lines):
        if 'license' in line.lower():
            print(f"Line {i+1}: {line}")
else:
    print("✅ NO 'license-file' found in entire metadata!")
