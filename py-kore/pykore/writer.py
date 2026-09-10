"""
PySpark-compatible DataFrameWriter for KORE Engine.

Mirrors ``pyspark.sql.DataFrameWriter`` so that ``df.write.parquet(path)``
works as expected.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from .dataframe import KoreDataFrame


class DataFrameWriter:
    """Write-side API returned by ``DataFrame.write``."""

    def __init__(self, df: "KoreDataFrame"):
        self._df = df
        self._format: str = "parquet"
        self._mode: str = "error"
        self._options: dict[str, str] = {}
        self._partition_by: list[str] = []

    # -- builder methods -------------------------------------------------------

    def format(self, fmt: str) -> "DataFrameWriter":
        """Set the output format (e.g. ``"parquet"``, ``"csv"``, ``"json"``)."""
        self._format = fmt.lower()
        return self

    def mode(self, saveMode: str) -> "DataFrameWriter":  # noqa: N803
        """Set the save mode: ``"overwrite"``, ``"append"``, ``"error"``, ``"ignore"``."""
        self._mode = saveMode.lower()
        return self

    def option(self, key: str, value: Any) -> "DataFrameWriter":
        self._options[key] = str(value)
        return self

    def options(self, **kwargs: Any) -> "DataFrameWriter":
        for k, v in kwargs.items():
            self._options[k] = str(v)
        return self

    def partitionBy(self, *cols: str) -> "DataFrameWriter":  # noqa: N802
        self._partition_by = list(cols)
        return self

    # -- terminal write methods ------------------------------------------------

    def parquet(self, path: str) -> None:
        """Write the DataFrame to Parquet format."""
        self._format = "parquet"
        self.save(path)

    def csv(self, path: str, **kwargs: Any) -> None:
        """Write the DataFrame to CSV format."""
        self._format = "csv"
        self._options.update({k: str(v) for k, v in kwargs.items()})
        self.save(path)

    def json(self, path: str) -> None:
        """Write the DataFrame to JSON format."""
        self._format = "json"
        self.save(path)

    def save(self, path: str) -> None:
        """Execute the write to *path* using the configured format and mode."""
        engine = self._df._get_engine()
        if engine is None:
            raise RuntimeError(
                "No KORE engine available — cannot write data. "
                "Build and install the kore_engine native module."
            )

        sql = self._df._build_sql()
        if self._format == "parquet":
            engine.write_parquet(sql, path)
        elif self._format == "csv":
            raise NotImplementedError(
                "CSV write not yet supported by the KORE native engine. "
                "Use .toPandas().to_csv() as a workaround."
            )
        elif self._format == "json":
            raise NotImplementedError(
                "JSON write not yet supported by the KORE native engine. "
                "Use .toPandas().to_json() as a workaround."
            )
        else:
            raise ValueError(f"Unsupported write format: {self._format}")

    def saveAsTable(self, name: str) -> None:  # noqa: N802
        """Materialise the DataFrame as a named in-memory table.

        This executes the query via SQL ``CREATE TABLE … AS SELECT …`` if the
        engine supports it, otherwise it collects and re-registers.
        """
        engine = self._df._get_engine()
        if engine is None:
            raise RuntimeError("No KORE engine available.")

        sql = self._df._build_sql()
        try:
            engine.sql(f"CREATE TABLE {name} AS {sql}")
        except Exception:
            rows = engine.sql(sql)
            if hasattr(engine, "_register_rows"):
                engine._register_rows(name, rows)
            else:
                raise
