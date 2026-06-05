import urllib.request
import json

try:
    data = json.loads(urllib.request.urlopen('https://pypi.org/pypi/kore-fileformat/json').read())
    print(f"Latest version: {data['info']['version']}")
    versions = list(data['releases'].keys())
    print(f"Total versions available: {len(versions)}")
    print(f"Last 5 versions: {', '.join(sorted(versions)[-5:])}")
except Exception as e:
    print(f"Error: {e}")
