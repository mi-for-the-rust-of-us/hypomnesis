# Windows `TCC` field check

A procedure for running `hmn` on a rented Windows box whose NVIDIA GPU is in
`TCC` mode. A `TCC` GPU is not a `WDDM` device, so `DXGI` and the `PDH`
counters do not see it, and only `NVML` and `nvidia-smi` do. It was first run
on 2026-10-27, on a Tesla V100; the results are in
[`docs/dogfooding-feedbacks/dogfooding-windows-tcc-field-check.md`](../../docs/dogfooding-feedbacks/dogfooding-windows-tcc-field-check.md).
Rerun it when a change touches the Windows paths that do not go through
`DXGI` or `PDH`.

The box is an OVHcloud Public Cloud `t1-45` (1× Tesla V100-PCIE-16GB, 8 vCPU,
45 GB) in `GRA7`, with the image *Windows Server 2022 Standard (Desktop) -
UEFI* (only OVH's `UEFI` images let the NVIDIA driver work), a Basic Public
IP, no gateway, no automatic backup, and hourly billing: about 0.98 € an hour
before VAT, the Windows licence included. A session takes about an hour, half
of it waiting for the instance.

## The kit

Build it at home, then copy the `kit` folder to the box. The binaries link the
C runtime statically, so a bare Windows Server runs them without the Visual
C++ redistributable.

```sh
RUSTFLAGS="-C target-feature=+crt-static" CARGO_TARGET_DIR=target/tcc-kit cargo build --release --bin hmn
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --manifest-path tools/cudahold/Cargo.toml
K=target/tcc-kit/kit
mkdir -p $K/fake
cp target/tcc-kit/release/hmn.exe $K/ && cp target/tcc-kit/release/hmn.exe $K/fake/
cp tools/cudahold/target/release/cudahold.exe tools/tcc-field-check/run-checks.ps1 $K/
clang -std=c11 -Wall -Wextra -Werror -shared -fms-runtime-lib=static -o $K/fake/nvml.dll tests/fixtures/nvml_fake.c
rm -f $K/fake/nvml.lib $K/fake/nvml.exp
```

| File | What it is |
|---|---|
| `hmn.exe` | `hmn` built from the commit under test (`hmn --version` prints the last released version until the bump) |
| `cudahold.exe` | [`tools/cudahold`](../cudahold): holds GPU memory through `nvcuda.dll`, which every NVIDIA driver installs, so `hmn` has a compute process to list without a `CUDA` toolkit or PyTorch |
| `fake\hmn.exe`, `fake\nvml.dll` | the same `hmn` beside the fake `NVML` of `tests/fixtures/nvml_fake.c`: Windows loads a DLL from the program's folder first, so this `hmn` talks to the fake while the real driver stays installed |
| `run-checks.ps1` | runs every check and writes each command, its stdout, its stderr and its exit code to `tcc-check.log` beside itself; Windows PowerShell 5.1 compatible |

## Once: the OVHcloud project

One OVHcloud account covers everything; GPU instances live in its **Public
Cloud** section (switch to it in the Control Panel's top bar), inside a
project. A first project with a payment method gets a free trial credit (200 €
for a month in 2026). A new project's quota is 6 vCPU, too few for a `t1-45`
("core quota 6 reached"): the console's *Augmenter mes quotas* only sells
non-refundable prepaid credit (100 € minimum), so ask support instead
(*Contacter le support*), naming the instance type, the region, the purpose
and a public URL for the project; 16 vCPU, 64 GB and 2 instances were granted
for free in under two hours. The project, its quota and its credit outlive
every instance.

## Steps

1. **Launch the instance** with the settings above (*Instances*, *Créer une
   instance*). Provisioning took about 15 minutes. Note its public IP.
2. **Set the Administrator password** in the instance's **VNC console**
   (click the instance's name; the `⋮` menu does not have it). Keep the
   defaults on Windows' first screen (English, US keyboard). The VNC console
   and the US layout may not type what a French keyboard means, so use only
   letters that sit at the same place on AZERTY and QWERTY
   (`b c d e f g h i j k l n o p r s t u v x y`, with Shift for capitals) and
   digits from the numeric keypad, and no symbols.
3. **Connect from home** with Remote Desktop (`mstsc`) to the public IP as
   `Administrator`, and accept the self-signed certificate. Close Server
   Manager. Copy the `kit` folder from your Explorer and paste it into the
   session's Explorer (`Documents` will do: the script finds its files beside
   itself).
4. **Install the NVIDIA driver.** The image has none (`nvidia-smi` is not
   found). The data center driver 582.70 (`R580`, the last branch that
   supports Volta) lists Windows Server 2022 and the V100; in PowerShell
   (Admin), use `curl.exe`, not PowerShell's `curl` alias:
   ```
   curl.exe -L -o C:\Users\Administrator\Downloads\nvidia-582.70.exe https://us.download.nvidia.com/tesla/582.70/582.70-data-center-tesla-desktop-winserver-2022-2025-dch-international.exe
   C:\Users\Administrator\Downloads\nvidia-582.70.exe
   ```
   Choose *Express*. It needed no reboot.
5. **Check the driver model.** `nvidia-smi` shows `TCC` in the
   *Driver-Model* column, and `Disp.A` reads `Off`. The data center driver
   puts a Tesla card in `TCC` by default; if it says `WDDM`, run
   `nvidia-smi -g 0 -dm 1` and reboot. `WDDM` itself is not worth testing on
   this card: NVIDIA's release notes say `WDDM` features on data center GPUs
   need vGPU.
6. **Run the checks** (about a minute):
   `powershell -ExecutionPolicy Bypass -File C:\Users\Administrator\Documents\kit\run-checks.ps1`.
   If `cudahold` cannot get its memory, the log says why and the other
   sections still run.
7. **Bring `tcc-check.log` home** by copying it back through the session.
8. **Optional: a disabled card.** In PowerShell (Admin):
   ```
   $gpu = Get-PnpDevice -Class Display | Where-Object FriendlyName -like '*V100*'
   Disable-PnpDevice -InstanceId $gpu.InstanceId -Confirm:$false
   .\hmn.exe; "exit=$LASTEXITCODE"
   .\hmn.exe ps; "exit=$LASTEXITCODE"
   nvidia-smi; "exit=$LASTEXITCODE"
   Enable-PnpDevice -InstanceId $gpu.InstanceId -Confirm:$false
   .\hmn.exe; "exit=$LASTEXITCODE"
   ```
9. **Tear down.** Delete the instance (*Instances*, `⋮`, *Supprimer*: an
   instance that is only shut down is still billed), then the private network
   (*Network*, *Private Network*). Do not create backups or switch to monthly
   billing from that menu. Keep the project.

## What to look for

| Check | Expected | Seen on 2026-10-27 |
|---|---|---|
| `hmn` | one line, `GPU 0 [Tesla V100-…]` with free, total, reserved and the driver version, exit `0`; the name comes from `NVML`, since `DXGI` has no NVIDIA adapter; no line for the VM's display adapter | as expected; the figures match `nvidia-smi` to the MiB |
| `hmn ps`, idle | a header and `hmn: 0 GPU processes found.`, exit `0` (`1` with `--exit-status`) | as expected |
| `hmn ps` with the two holders | two rows whose VRAM matches `nvidia-smi --query-compute-apps`, `GPU` 0, `DEVICE` the V100's name, through the `nvidia-smi` fallback (`PDH` finds no adapter) | as expected, but NAME is the full path `nvidia-smi` prints, and SPILL reads `?` |
| `hmn watch` on the holders, auto-selected, `--filter holder` | their bytes, steady | as expected; spill tracking reported as a `PDH error` |
| `hmn watch` on PowerShell's own PID | NAME `powershell.exe`, from the direct lookup, at `0 MiB` | as expected |
| `hmn spill` | the child runs; spill not measurable | `hmn spill: spill not measurable (no tracker)`, after a `PDH error` line |
| fake `NVML`, `INIT=18` | `hmn: failed to count devices: NVML error: nvmlInit_v2 returned NVML_ERROR_LIB_RM_VERSION_MISMATCH (18)` and `hmn: ps failed to count devices: …`, both exit `2`: a real `DXGI` zero does not override a broken `NVML` | as expected |
| fake `NVML`, `INIT=9` and `INIT=28` | `hmn: no visible GPUs.` and `hmn: 0 GPU processes found (no GPU to list).`, exit `0` | as expected |
| fake `NVML`, one device | `GPU 0 [Fake Tesla]: free 15872 MiB / 16384 MiB (512 MiB reserved), driver 999.99` | as expected |
| disabled card | `hmn: no visible GPUs.`, exit `0` (`NVML` says its driver is not loaded; nothing can run there) | as expected; `nvidia-smi` exits `9` |
