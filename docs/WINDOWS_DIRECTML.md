# Windows DirectML texture-transport boundary

LiveBlock's current Windows detector prefers ONNX Runtime's DirectML execution
provider, but its input remains a CPU-created float32 RGB NCHW tensor. This is
**not** D3D texture binding, shared-texture inference, or proof that graph nodes
executed on a physical GPU.

## Version-pinned API findings

The release branch pins ONNX Runtime and the Rust `ort` wrapper to 1.18.1 / rc.4.
The upstream 1.18.1 contract establishes that:

- DirectML's extension API accepts an `ID3D12Resource`, creates a DML allocation
  wrapper, requires explicit wrapper release, and can recover the underlying
  D3D12 resource ([`dml_provider_factory.h` lines 130–146](https://github.com/microsoft/onnxruntime/blob/v1.18.1/include/onnxruntime/core/providers/dml/dml_provider_factory.h#L130-L146)).
- ONNX Runtime's own test keeps that wrapper alive with the `OrtValue`, derives
  tensor byte size from the D3D12 buffer, and constructs the tensor over the
  wrapped allocation ([`test_inference.cc` lines 268–297](https://github.com/microsoft/onnxruntime/blob/v1.18.1/onnxruntime/test/shared_lib/test_inference.cc#L268-L297)).
- I/O binding avoids host/device copies only when the application has already
  placed changing inputs on the target device; ordinary CPU values are copied
  during binding/run ([official I/O binding documentation](https://onnxruntime.ai/docs/performance/tune-performance/iobinding.html)).
- DirectML requires sequential session execution and disabled memory-pattern
  optimization ([official DirectML provider documentation](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)).

The pinned Rust wrapper exposes DirectML provider registration, generic DML
`MemoryInfo`, unsafe raw tensor construction, and `IoBinding`, but it does not
wrap `OrtDmlApi::CreateGPUAllocationFromD3DResource` or the corresponding
lifetime protocol. Using `TensorRefMut::from_raw` directly on an
`ID3D11Texture2D` pointer would be type-incorrect: the DML allocation pointer is
an EP wrapper around a **D3D12 buffer resource**, not a D3D11 texture COM pointer.

## LiveBlock's actual data path

1. Windows Graphics Capture supplies a BGRA D3D11 texture.
2. The callback copies the newest frame into an application-owned D3D11 texture.
3. The worker reads packed BGRA to CPU memory.
4. Shared Rust preprocessing letterboxes, converts BGRA to RGB, normalizes, and
   creates a 1×3×640×640 float32 NCHW tensor.
5. A DirectML-registered ORT session consumes that CPU tensor, or an explicit CPU
   session is constructed if DirectML registration/model loading fails.

`directml_registered_cpu_uploaded_tensor` means only that DirectML registration
and session/model creation succeeded. `cpu_after_directml_load_failure` means the
separate CPU session loaded after the DirectML attempt failed. Neither status is
physical-GPU execution evidence.

## Current-source feasibility findings

The remaining work is not a safe incremental patch on the existing texture. The
capture session creates a plain D3D11 device with `D3D11CreateDevice(None,
D3D_DRIVER_TYPE_HARDWARE, ...)`; it does not select or persist an adapter LUID.
The application-owned BGRA texture has `MiscFlags: 0`, so it is unshared, and the
Windows crate does not enable the Direct3D12 or Direct3D11On12 API surfaces. The
DirectML provider also uses its default device ID rather than a device mapped to
the capture adapter. On a mixed-adapter system those defaults are not evidence
that capture and inference even selected the same physical device.

D3D11On12 cannot retroactively unwrap an arbitrary resource created by this plain
D3D11 device. A viable D3D11On12 design must start from the monitor's DXGI adapter,
create the D3D12 device and queue there, create the D3D11On12 device over that
queue, and give that D3D11 device to the WGC frame pool. That is a capture-device
and lifecycle migration, not a pointer cast. A shared-handle alternative still
requires shareable resource creation, same-adapter validation, explicit fence/
keyed synchronization, access-state transitions, and device-loss ownership.

The pinned `ort` rc.4 `IoBinding::bind_input` documentation describes a copy at
bind time for ordinary values and is optimized for inputs reused across runs;
LiveBlock's frame input changes every run. `ort` rc.4 and `ort-sys` rc.4 do not
expose `OrtDmlApi` or `CreateGPUAllocationFromD3DResource`, although a later
wrapper revision is irrelevant to the pinned runtime ABI. A correct implementation
therefore needs a reviewed version-matched unsafe shim, a D3D12 **buffer** holding
the GPU-preprocessed tensor, a wrapper whose lifetime outlives the bound
`OrtValue`, and a per-frame I/O binding path. Binding the BGRA texture or its COM
pointer directly would remain invalid.

`platform/windows/directml-transport-readiness.json` is the machine-readable
no-go/go record. `tools/verify_windows_directml_readiness.py` binds the pinned
runtime/crate versions, current source facts, confirmed cancellation/backpressure
seams, and eight required gates. It refuses `ready` unless every gate has
hash-bound repository evidence and the hardware matrix contains AMD, Intel,
NVIDIA, and CPU runs. The current record is intentionally `blocked` / `defer`
with 0/8 gates passed; this does not erase the already implemented CPU fallback
and per-run cancellation seams.

## Required architecture before this row can close

A correct device-input implementation needs one reviewed transaction spanning:

1. create D3D12/DirectML devices and command queue on the selected capture
   adapter, not an unrelated default adapter;
2. bridge the application-owned D3D11 frame to D3D12 with explicit keyed/shared
   synchronization or D3D11On12;
3. run GPU preprocessing into a contiguous float32 NCHW **D3D12 buffer** (a BGRA
   texture cannot satisfy the model tensor contract directly);
4. call the version-matched `OrtDmlApi`, wrap the buffer, bind it with ORT I/O
   binding, and keep the resource, allocation wrapper, queue, fences, and session
   alive through inference;
5. preserve per-run termination, capture-generation invalidation, device-loss
   recovery, adapter changes, and bounded caller waits;
6. read back only the small model output for shared decoding, unless
   postprocessing also moves to a parity-tested GPU implementation; and
7. prove CPU/device preprocessing parity and run NVIDIA, AMD, and Intel hardware
   tests before changing capability language.

That architecture is not safely provided by `ort` rc.4 and is not implemented in
this branch. A bespoke `ort-sys`/D3D12 bridge would add a second unsafe runtime
ABI and substantial device-loss/lifetime surface before a promoted ONNX model
exists. It would also lack the exact ONNX/CoreML parity artifact needed to prove
that GPU preprocessing preserved the promoted model contract. The release
decision is therefore to keep the current explicit CPU-input path, enforce
DirectML's required session options, make registration failure and CPU fallback
truthful, and leave the texture-transport checklist item open. Revisit only after
a promoted ONNX artifact exists and the readiness record can accumulate real
parity and device evidence gate by gate.
