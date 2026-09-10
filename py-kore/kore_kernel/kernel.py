"""KORE SQL Jupyter Kernel implementation."""

from __future__ import annotations

import re
import sys
import time
import traceback
from io import StringIO
from typing import Any

from ipykernel.kernelbase import Kernel

from pykore import SparkSession
from .formatters import (
    datablock_to_html,
    datablock_to_text,
    format_schema,
    suggest_chart,
)


class KoreKernel(Kernel):
    implementation = "kore"
    implementation_version = "0.1"
    language = "sql"
    language_version = "1.0"
    language_info = {
        "name": "kore-sql",
        "mimetype": "text/x-sql",
        "file_extension": ".sql",
    }
    banner = "KORE Engine — Interactive SQL"

    def __init__(self, **kwargs: Any):
        super().__init__(**kwargs)
        self.spark = SparkSession.builder.appName("kore-notebook").getOrCreate()

    def do_execute(
        self,
        code: str,
        silent: bool,
        store_history: bool = True,
        user_expressions: dict[str, Any] | None = None,
        allow_stdin: bool = False,
    ) -> dict[str, Any]:
        code = code.strip()
        if not code:
            return self._ok()

        try:
            if code.startswith("%%python"):
                return self._exec_python(code[len("%%python"):].strip(), silent)
            if code.startswith("%"):
                return self._exec_magic(code, silent)
            return self._exec_sql(code, silent)
        except Exception as exc:
            return self._error(exc)

    def _exec_sql(self, code: str, silent: bool) -> dict[str, Any]:
        """Execute a SQL query and display results."""
        df = self.spark.sql(code)
        rows = df.collect()

        if silent:
            return self._ok()

        columns = list(rows[0].keys()) if rows else df.columns
        html = datablock_to_html(columns, rows)
        text = datablock_to_text(columns, rows)

        self.send_response(
            self.iopub_socket,
            "display_data",
            {
                "data": {"text/html": html, "text/plain": text},
                "metadata": {},
            },
        )

        chart = suggest_chart(columns, rows)
        if chart:
            self.send_response(
                self.iopub_socket,
                "display_data",
                {
                    "data": {"text/html": chart, "text/plain": "(chart)"},
                    "metadata": {},
                },
            )

        return self._ok()

    def _exec_magic(self, code: str, silent: bool) -> dict[str, Any]:
        """Handle magic commands."""
        parts = code.split(None, 1)
        command = parts[0].lower()
        args = parts[1].strip() if len(parts) > 1 else ""

        if command == "%tables":
            return self._magic_tables(silent)
        elif command == "%schema":
            return self._magic_schema(args, silent)
        elif command == "%load_csv":
            return self._magic_load_csv(args, silent)
        elif command == "%load_parquet":
            return self._magic_load_parquet(args, silent)
        elif command == "%explain":
            return self._magic_explain(args, silent)
        elif command == "%time":
            return self._magic_time(args, silent)
        else:
            return self._error(ValueError(f"Unknown magic command: {command}"))

    def _magic_tables(self, silent: bool) -> dict[str, Any]:
        """List registered tables."""
        tables = self.spark.catalog.listTables()
        if silent:
            return self._ok()

        if not tables:
            self._send_text("No tables registered.")
        else:
            lines = ["Registered tables:", ""]
            for t in tables:
                lines.append(f"  • {t}")
            self._send_text("\n".join(lines))
        return self._ok()

    def _magic_schema(self, table_name: str, silent: bool) -> dict[str, Any]:
        """Show table schema."""
        if not table_name:
            return self._error(ValueError("%schema requires a table name"))

        df = self.spark.table(table_name)
        dtypes = df.dtypes
        columns = [name for name, _ in dtypes]
        text = format_schema(columns, dtypes)

        if not silent:
            self._send_text(text)
        return self._ok()

    def _magic_load_csv(self, args: str, silent: bool) -> dict[str, Any]:
        """Load CSV file: %load_csv path AS tablename"""
        match = re.match(r"(.+?)\s+AS\s+(\w+)", args, re.IGNORECASE)
        if not match:
            return self._error(
                ValueError("Usage: %load_csv <path> AS <tablename>")
            )

        path = match.group(1).strip().strip("'\"")
        table_name = match.group(2).strip()

        engine = self.spark._engine
        engine.load_csv(path, table_name)

        if not silent:
            self._send_text(f"Loaded '{path}' as table '{table_name}'.")
        return self._ok()

    def _magic_load_parquet(self, args: str, silent: bool) -> dict[str, Any]:
        """Load Parquet file: %load_parquet path AS tablename"""
        match = re.match(r"(.+?)\s+AS\s+(\w+)", args, re.IGNORECASE)
        if not match:
            return self._error(
                ValueError("Usage: %load_parquet <path> AS <tablename>")
            )

        path = match.group(1).strip().strip("'\"")
        table_name = match.group(2).strip()

        engine = self.spark._engine
        engine.read_parquet(path, table_name)

        if not silent:
            self._send_text(f"Loaded '{path}' as table '{table_name}'.")
        return self._ok()

    def _magic_explain(self, query: str, silent: bool) -> dict[str, Any]:
        """Show query execution plan."""
        if not query:
            return self._error(ValueError("%explain requires a SQL query"))

        df = self.spark.sql(query)
        sql = df._build_sql()

        if not silent:
            self._send_text(f"== Physical Plan ==\n{sql}")
        return self._ok()

    def _magic_time(self, query: str, silent: bool) -> dict[str, Any]:
        """Time a query execution."""
        if not query:
            return self._error(ValueError("%time requires a SQL query"))

        start = time.perf_counter()
        df = self.spark.sql(query)
        rows = df.collect()
        elapsed = time.perf_counter() - start

        if not silent:
            columns = list(rows[0].keys()) if rows else df.columns
            html = datablock_to_html(columns, rows)
            text = datablock_to_text(columns, rows)
            timing = f"\n⏱ Query executed in {elapsed:.4f}s ({len(rows)} rows)"

            self.send_response(
                self.iopub_socket,
                "display_data",
                {
                    "data": {
                        "text/html": html + f"<p><em>{timing.strip()}</em></p>",
                        "text/plain": text + timing,
                    },
                    "metadata": {},
                },
            )
        return self._ok()

    def _exec_python(self, code: str, silent: bool) -> dict[str, Any]:
        """Execute Python code with access to the spark session."""
        local_ns: dict[str, Any] = {"spark": self.spark}

        old_stdout = sys.stdout
        sys.stdout = captured = StringIO()
        try:
            exec(code, {"__builtins__": __builtins__}, local_ns)  # noqa: S102
        finally:
            sys.stdout = old_stdout

        output = captured.getvalue()
        if output and not silent:
            self._send_text(output)
        return self._ok()

    def _send_text(self, text: str) -> None:
        """Send plain text to the notebook output."""
        self.send_response(
            self.iopub_socket,
            "stream",
            {"name": "stdout", "text": text + "\n"},
        )

    def _ok(self) -> dict[str, Any]:
        return {
            "status": "ok",
            "execution_count": self.execution_count,
            "payload": [],
            "user_expressions": {},
        }

    def _error(self, exc: Exception) -> dict[str, Any]:
        tb = traceback.format_exception(type(exc), exc, exc.__traceback__)
        self.send_response(
            self.iopub_socket,
            "stream",
            {"name": "stderr", "text": f"Error: {exc}\n"},
        )
        return {
            "status": "error",
            "ename": type(exc).__name__,
            "evalue": str(exc),
            "traceback": tb,
        }

    def do_is_complete(self, code: str) -> dict[str, str]:
        """Check if code input is complete."""
        code = code.strip()
        if not code:
            return {"status": "incomplete", "indent": ""}
        if code.endswith(";") or code.startswith("%"):
            return {"status": "complete"}
        return {"status": "unknown"}

    def do_complete(self, code: str, cursor_pos: int) -> dict[str, Any]:
        """Provide basic completions for table names and magic commands."""
        prefix = code[:cursor_pos]
        token = prefix.split()[-1] if prefix.split() else ""

        matches: list[str] = []
        if token.startswith("%"):
            magics = ["%tables", "%schema", "%load_csv", "%load_parquet", "%explain", "%time", "%%python"]
            matches = [m for m in magics if m.startswith(token)]
        else:
            tables = self.spark.catalog.listTables()
            matches = [t for t in tables if t.startswith(token)]

        return {
            "status": "ok",
            "matches": matches,
            "cursor_start": cursor_pos - len(token),
            "cursor_end": cursor_pos,
            "metadata": {},
        }
