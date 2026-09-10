"""Install the KORE SQL kernel into Jupyter."""

from __future__ import annotations

import json
import os
import sys
import tempfile
import shutil


KERNEL_SPEC = {
    "argv": [sys.executable, "-m", "kore_kernel", "-f", "{connection_file}"],
    "display_name": "KORE SQL",
    "language": "sql",
}


def install_kernel(user: bool = True, prefix: str | None = None) -> str:
    """Install the KORE kernel spec to Jupyter.

    Parameters
    ----------
    user : bool
        Install for the current user only (default True).
    prefix : str, optional
        Install to a custom prefix directory.

    Returns
    -------
    str
        Path where the kernel was installed.
    """
    from jupyter_client.kernelspec import KernelSpecManager

    kernel_dir = tempfile.mkdtemp(suffix="_kore_kernel")
    try:
        spec_path = os.path.join(kernel_dir, "kernel.json")
        with open(spec_path, "w", encoding="utf-8") as f:
            json.dump(KERNEL_SPEC, f, indent=2)

        ksm = KernelSpecManager()
        dest = ksm.install_kernel_spec(
            kernel_dir,
            kernel_name="kore-sql",
            user=user,
            prefix=prefix,
        )
    finally:
        shutil.rmtree(kernel_dir, ignore_errors=True)

    return dest


def main() -> None:
    """CLI entry point for kernel installation."""
    import argparse

    parser = argparse.ArgumentParser(description="Install KORE SQL Jupyter kernel")
    parser.add_argument(
        "--user",
        action="store_true",
        default=True,
        help="Install for current user (default)",
    )
    parser.add_argument(
        "--sys-prefix",
        action="store_true",
        help="Install to sys.prefix (for virtualenvs)",
    )
    parser.add_argument(
        "--prefix",
        type=str,
        default=None,
        help="Install to a custom prefix",
    )
    args = parser.parse_args()

    prefix = None
    user = args.user
    if args.sys_prefix:
        prefix = sys.prefix
        user = False
    elif args.prefix:
        prefix = args.prefix
        user = False

    dest = install_kernel(user=user, prefix=prefix)
    print(f"KORE SQL kernel installed to: {dest}")


if __name__ == "__main__":
    main()
