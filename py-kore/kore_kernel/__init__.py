"""KORE SQL Jupyter Kernel — interactive SQL queries in notebooks."""

try:
    from .kernel import KoreKernel
except ImportError:
    KoreKernel = None  # type: ignore[assignment, misc]

__all__ = ["KoreKernel"]
