import kore_fileformat
print("✅ kore-fileformat v1.2.3 installed successfully")
print(f"Module location: {kore_fileformat.__file__}")
print(f"Available: {[x for x in dir(kore_fileformat) if not x.startswith('_')][:5]}")
