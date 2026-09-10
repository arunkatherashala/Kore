"""Entry point for ``python -m kore_kernel``."""

from ipykernel.kernelapp import IPKernelApp
from .kernel import KoreKernel

IPKernelApp.launch_instance(kernel_class=KoreKernel)
