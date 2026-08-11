FROM python:3.12-slim

WORKDIR /app

COPY kore-python/kore_fileformat.py /app/
COPY kore-python/kore_duckdb.py /app/
COPY kore-python/kore_athena.py /app/

RUN pip install --no-cache-dir pyarrow polars duckdb

EXPOSE 8080

CMD ["python", "-c", "import kore_fileformat as kf; print(f'KORE FileFormat v{kf.__version__} ready'); import http.server; http.server.HTTPServer(('0.0.0.0', 8080), http.server.SimpleHTTPRequestHandler).serve_forever()"]
