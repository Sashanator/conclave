# CUDA setup on Windows 10

Conclave's CUDA builds ship only the tiny `cudart64_12.dll` (about 0.5 MB). The big cuBLAS libraries (about 600 MB) come from the CUDA 12 toolkit installed on your PC, which keeps the installer small. Without the toolkit, or without an NVIDIA driver, Conclave still starts and transcribes on the CPU.

## 1. Check your NVIDIA driver

In PowerShell, run:

```powershell
nvidia-smi
```

In the top-right corner, look for `CUDA UMD Version: X.Y` (newer drivers) or `CUDA Version: X.Y` (older drivers). It must be **12.8 or higher**. A driver that shows 13.3, for example, is fine, because it runs CUDA 12 apps. If it's lower, update the driver from the NVIDIA App or https://www.nvidia.com/drivers first.

## 2. Download CUDA Toolkit 12.8

Go to https://developer.nvidia.com/cuda-12-8-0-download-archive and choose **Windows → x86_64 → 10 → exe (local)**.

Stay on **12.x**. Don't install 13.x. See [Why CUDA 12 and not 13](#why-cuda-12-and-not-13) at the end.

## 3. Install: Custom, not Express

Run the installer and choose **Custom (Advanced)**.

- **Keep:** CUDA → Runtime → Libraries. This is the only box to check under CUDA.
- **Uncheck:**
  - Development. It's a few GB of compiler and headers that Conclave doesn't need.
  - Driver components, so the installer doesn't replace your newer driver.
  - Nsight (all entries).
  - Visual Studio Integration.
  - Runtime → Demo Suite, and Documentation.
  - Other components (PhysX).
  - NVIDIA App components. The bundled NVIDIA App can be older than the one you have, and it would downgrade it.

## 4. Add CUDA to Path

The Libraries-only install sets `CUDA_PATH`, but it does not add `bin` to `Path`. Add it yourself. Editing System variables needs an administrator account.

1. Press Start, type `environment variables`, and open **Edit the system environment variables**. Then click **Environment Variables…**.
2. Under **System variables**, select `Path` and click **Edit**.
3. Click **New** and type `%CUDA_PATH%\bin`.
4. Click **OK** in each window.

Then open a **new** PowerShell window and run:

```powershell
echo $env:CUDA_PATH          # C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8
where.exe cublas64_12.dll    # ...\CUDA\v12.8\bin\cublas64_12.dll
where.exe cublasLt64_12.dll
```

If `where.exe` finds nothing, check that `%CUDA_PATH%\bin` is in the System `Path` list (sub-step 3 above), then open a new PowerShell window and try again.

## 5. Keep it installed

The CUDA build needs these two DLLs from the toolkit:
- `cublas64_12.dll`
- `cublasLt64_12.dll`

`cudart64_12.dll` ships with Conclave.

If you uninstall the toolkit or replace it with 13.x, Conclave falls back to the CPU and shows "CUDA 12 runtime not found". To fix it, reinstall a 12.x toolkit.

## 6. Restart Conclave

1. Quit Conclave from the tray icon. Closing the window is not enough.
2. Start Conclave from the Start menu.

A Conclave started by the installer's "Run Conclave" checkbox keeps the old `Path`, so always start it from the Start menu.

## 7. Check that Conclave uses the GPU (CUDA builds)

1. Choose a **Whisper** model in Settings → Transcription (Parakeet/Lightning always runs on the CPU). The settings should read **"Whisper running on: CUDA (NVIDIA GeForce RTX 4070 Ti SUPER)"**. If Settings still says CPU, sign out of Windows, sign back in, and start Conclave again.
2. During a recording, open **Task Manager → Performance → GPU**.
3. Switch one graph to **Cuda**. It should be active while Conclave transcribes.

## Why CUDA 12 and not 13

- Whisper (whisper.cpp) is far more widely built and tested on CUDA 12.
- CUDA 13 gives no speedup for Whisper on an RTX 40-series card.
- CUDA 13's DLLs are named `*64_13.dll`, so a 13-only install is not found.
- A 12.x and a 13.x toolkit can be installed side by side.
