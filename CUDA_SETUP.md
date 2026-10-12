# CUDA setup on Windows 10

Conclave's GPU builds (Phase 1 onward) ship only the tiny `cudart64_12.dll` (about 0.5 MB). The big cuBLAS libraries (about 600 MB) come from the CUDA 12 toolkit installed on your PC, which keeps the installer small. Without the toolkit, or without an NVIDIA driver, Conclave still starts and transcribes on the CPU.

> v0.1.0 is a CPU-only build. It doesn't need CUDA, and installing CUDA now does no harm.

## 1. Check your NVIDIA driver

In PowerShell, run:

```powershell
nvidia-smi
```

The top-right corner shows `CUDA Version: X.Y`. It must be **12.8 or higher**. If it's lower, update the driver from the NVIDIA App or https://www.nvidia.com/drivers first.

## 2. Download CUDA Toolkit 12.8

Go to https://developer.nvidia.com/cuda-12-8-0-download-archive and choose **Windows → x86_64 → 10 → exe (local)**.

Stay on **12.x**. Don't install 13.x: its DLLs have different names (`*64_13.dll`), and Conclave is built against CUDA 12.

## 3. Install: Custom, not Express

Run the installer and choose **Custom (Advanced)**.

- **Keep:** CUDA → Runtime → Libraries. This is what Conclave needs.
- **Uncheck:**
  - Driver components, so the installer doesn't replace your newer driver.
  - Nsight (all entries).
  - Visual Studio Integration.
  - Samples and Documentation.

## 4. Verify

Open a **new** PowerShell window and run:

```powershell
echo $env:CUDA_PATH          # C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8
where.exe cublas64_12.dll    # ...\CUDA\v12.8\bin\cublas64_12.dll
where.exe cublasLt64_12.dll
```

If `where.exe` finds nothing:
1. Open **System Properties → Environment Variables**.
2. Edit `Path` (under System variables) and add `%CUDA_PATH%\bin`.
3. Open a new PowerShell window and check again.

## 5. Keep it installed

The CUDA build needs these two DLLs from the toolkit:
- `cublas64_12.dll`
- `cublasLt64_12.dll`

`cudart64_12.dll` ships with Conclave.

If you uninstall the toolkit or replace it with 13.x, Conclave falls back to the CPU and shows "CUDA 12 runtime not found". To fix it, reinstall a 12.x toolkit.

## 6. Check that Conclave uses the GPU (CUDA builds)

1. Open Conclave's transcription settings. They should read **"Whisper running on: CUDA (NVIDIA GeForce RTX 4070 Ti SUPER)"**.
2. During a recording, open **Task Manager → Performance → GPU**.
3. Switch one graph to **Cuda**. It should be active while Conclave transcribes.
