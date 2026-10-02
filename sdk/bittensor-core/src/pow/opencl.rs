//! Minimal dynamically loaded OpenCL 1.2 host. Kernels use bounded buffers and
//! one in-order queue per device; every answer is recomputed on the CPU.

use super::{seal, validate_batch, Solution, PREFIX_LEN};
use libloading::Library;
use sp_core::U256;
use std::{ffi::c_void, ptr, sync::Arc};

type Handle = *mut c_void;
type Status = i32;
type Notify = Option<unsafe extern "C" fn(*const i8, *const c_void, usize, *mut c_void)>;

macro_rules! opencl_api {
    ($( $name:ident : $ty:ty ),* $(,)?) => {
        #[allow(non_snake_case)]
        struct Api {
            $( $name: $ty, )*
            // Function pointers must never outlive the loaded library.
            _library: Library,
        }
        impl Api {
            fn load() -> Result<Arc<Self>, String> {
                #[cfg(target_os = "windows")]
                let names = ["OpenCL.dll"];
                #[cfg(target_os = "macos")]
                let names = ["/System/Library/Frameworks/OpenCL.framework/OpenCL"];
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                let names = ["libOpenCL.so.1", "libOpenCL.so"];
                let mut last = String::new();
                for name in names {
                    // SAFETY: load the vendor's system OpenCL runtime. Typed
                    // symbols match the OpenCL 1.2 C ABI and library is retained.
                    match unsafe { Library::new(name) } {
                        Ok(library) => {
                            return unsafe {
                                Ok(Arc::new(Self {
                                    $( $name: *library.get::<$ty>(concat!(stringify!($name), "\0").as_bytes())
                                        .map_err(|error| format!("OpenCL symbol: {error}"))?, )*
                                    _library: library,
                                }))
                            };
                        }
                        Err(error) => last = error.to_string(),
                    }
                }
                Err(format!("OpenCL GPU runtime unavailable: {last}"))
            }
        }
    };
}

opencl_api! {
    clGetPlatformIDs: unsafe extern "C" fn(u32, *mut Handle, *mut u32) -> Status,
    clGetDeviceIDs: unsafe extern "C" fn(Handle, u64, u32, *mut Handle, *mut u32) -> Status,
    clGetDeviceInfo: unsafe extern "C" fn(Handle, u32, usize, *mut c_void, *mut usize) -> Status,
    clCreateContext: unsafe extern "C" fn(*const isize, u32, *const Handle, Notify, *mut c_void, *mut Status) -> Handle,
    clCreateCommandQueue: unsafe extern "C" fn(Handle, Handle, u64, *mut Status) -> Handle,
    clCreateProgramWithSource: unsafe extern "C" fn(Handle, u32, *const *const i8, *const usize, *mut Status) -> Handle,
    clBuildProgram: unsafe extern "C" fn(Handle, u32, *const Handle, *const i8, Option<unsafe extern "C" fn(Handle, *mut c_void)>, *mut c_void) -> Status,
    clGetProgramBuildInfo: unsafe extern "C" fn(Handle, Handle, u32, usize, *mut c_void, *mut usize) -> Status,
    clCreateKernel: unsafe extern "C" fn(Handle, *const i8, *mut Status) -> Handle,
    clCreateBuffer: unsafe extern "C" fn(Handle, u64, usize, *mut c_void, *mut Status) -> Handle,
    clSetKernelArg: unsafe extern "C" fn(Handle, u32, usize, *const c_void) -> Status,
    clEnqueueWriteBuffer: unsafe extern "C" fn(Handle, Handle, u32, usize, usize, *const c_void, u32, *const Handle, *mut Handle) -> Status,
    clEnqueueReadBuffer: unsafe extern "C" fn(Handle, Handle, u32, usize, usize, *mut c_void, u32, *const Handle, *mut Handle) -> Status,
    clEnqueueNDRangeKernel: unsafe extern "C" fn(Handle, Handle, u32, *const usize, *const usize, *const usize, u32, *const Handle, *mut Handle) -> Status,
    clFlush: unsafe extern "C" fn(Handle) -> Status,
    clFinish: unsafe extern "C" fn(Handle) -> Status,
    clReleaseMemObject: unsafe extern "C" fn(Handle) -> Status,
    clReleaseKernel: unsafe extern "C" fn(Handle) -> Status,
    clReleaseProgram: unsafe extern "C" fn(Handle) -> Status,
    clReleaseCommandQueue: unsafe extern "C" fn(Handle) -> Status,
    clReleaseContext: unsafe extern "C" fn(Handle) -> Status,
}

fn check(status: Status, operation: &str) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("OpenCL {operation} failed ({status})"))
    }
}

#[derive(Debug, Clone)]
pub struct GpuDevice {
    pub id: usize,
    pub name: String,
    pub vendor: String,
}

fn device_text(api: &Api, device: Handle, param: u32) -> Result<String, String> {
    let mut size = 0;
    // SAFETY: size query and a bounded allocation for the OpenCL device string.
    unsafe {
        check(
            (api.clGetDeviceInfo)(device, param, 0, ptr::null_mut(), &mut size),
            "device info size",
        )?;
        if size == 0 || size > 65_536 {
            return Err("invalid OpenCL device info size".into());
        }
        let mut bytes = vec![0u8; size];
        check(
            (api.clGetDeviceInfo)(
                device,
                param,
                size,
                bytes.as_mut_ptr().cast(),
                ptr::null_mut(),
            ),
            "device info",
        )?;
        Ok(String::from_utf8_lossy(&bytes)
            .trim_end_matches('\0')
            .to_owned())
    }
}

fn discover(api: &Api) -> Result<Vec<(Handle, GpuDevice)>, String> {
    let mut count = 0;
    // SAFETY: OpenCL writes at most the requested number of handles into the
    // allocated vectors. Platform/device handles belong to the loaded runtime.
    unsafe {
        let status = (api.clGetPlatformIDs)(0, ptr::null_mut(), &mut count);
        if status == -1001 {
            return Ok(Vec::new());
        }
        check(status, "platform discovery")?;
        if count > 64 {
            return Err("too many OpenCL platforms".into());
        }
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut platforms = vec![ptr::null_mut(); count as usize];
        check(
            (api.clGetPlatformIDs)(count, platforms.as_mut_ptr(), ptr::null_mut()),
            "platform discovery",
        )?;
        let mut result = Vec::new();
        for platform in platforms {
            let mut n = 0;
            let status = (api.clGetDeviceIDs)(platform, 4, 0, ptr::null_mut(), &mut n);
            if status == -1 {
                continue;
            } // CL_DEVICE_NOT_FOUND
            check(status, "GPU discovery")?;
            if n > 64 {
                return Err("too many OpenCL GPUs on one platform".into());
            }
            if n == 0 {
                continue;
            }
            let mut devices = vec![ptr::null_mut(); n as usize];
            check(
                (api.clGetDeviceIDs)(platform, 4, n, devices.as_mut_ptr(), ptr::null_mut()),
                "GPU discovery",
            )?;
            for device in devices {
                let id = result.len();
                result.push((
                    device,
                    GpuDevice {
                        id,
                        name: device_text(api, device, 0x102b)?,
                        vendor: device_text(api, device, 0x102c)?,
                    },
                ));
            }
        }
        Ok(result)
    }
}

/// Discovery returns an empty list when the driver/runtime is absent. Explicit
/// miner construction returns a descriptive error instead of silently using CPU.
pub fn gpu_devices() -> Result<Vec<GpuDevice>, String> {
    let api = match Api::load() {
        Ok(api) => api,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(discover(&api)?.into_iter().map(|(_, info)| info).collect())
}

struct Worker {
    api: Arc<Api>,
    info: GpuDevice,
    context: Handle,
    queue: Handle,
    program: Handle,
    kernel: Handle,
    vector_kernel: Handle,
    buffers: [Handle; 5],
}

// SAFETY: each worker owns its context, in-order queue and buffers. The public
// miner requires &mut self for work, so no queue/resource is accessed concurrently.
unsafe impl Send for Worker {}

impl Worker {
    fn new(api: Arc<Api>, device: Handle, info: GpuDevice) -> Result<Self, String> {
        let mut worker = Self {
            api,
            info,
            context: ptr::null_mut(),
            queue: ptr::null_mut(),
            program: ptr::null_mut(),
            kernel: ptr::null_mut(),
            vector_kernel: ptr::null_mut(),
            buffers: [ptr::null_mut(); 5],
        };
        let mut status = 0;
        let source = include_str!("kernel.cl");
        let source_ptr = source.as_ptr().cast::<i8>();
        let length = source.len();
        // SAFETY: handles are retained in Worker and released even on partial
        // initialization failure. All source strings and lengths remain valid.
        unsafe {
            worker.context = (worker.api.clCreateContext)(
                ptr::null(),
                1,
                &device,
                None,
                ptr::null_mut(),
                &mut status,
            );
            check(status, "context")?;
            worker.queue =
                (worker.api.clCreateCommandQueue)(worker.context, device, 0, &mut status);
            check(status, "queue")?;
            worker.program = (worker.api.clCreateProgramWithSource)(
                worker.context,
                1,
                &source_ptr,
                &length,
                &mut status,
            );
            check(status, "program")?;
            let mut status = (worker.api.clBuildProgram)(
                worker.program,
                1,
                &device,
                c"-cl-std=CL1.2".as_ptr(),
                None,
                ptr::null_mut(),
            );
            if status != 0 {
                let mut size = 0;
                (worker.api.clGetProgramBuildInfo)(
                    worker.program,
                    device,
                    0x1183,
                    0,
                    ptr::null_mut(),
                    &mut size,
                );
                let mut bytes = vec![0u8; size.min(65_536)];
                if !bytes.is_empty() {
                    (worker.api.clGetProgramBuildInfo)(
                        worker.program,
                        device,
                        0x1183,
                        bytes.len(),
                        bytes.as_mut_ptr().cast(),
                        ptr::null_mut(),
                    );
                }
                return Err(format!(
                    "OpenCL kernel build failed ({status}): {}",
                    String::from_utf8_lossy(&bytes)
                ));
            }
            worker.kernel =
                (worker.api.clCreateKernel)(worker.program, c"mine".as_ptr(), &mut status);
            check(status, "kernel")?;
            worker.vector_kernel =
                (worker.api.clCreateKernel)(worker.program, c"seal_vector".as_ptr(), &mut status);
            check(status, "vector kernel")?;
            for (buffer, size) in worker.buffers.iter_mut().zip([PREFIX_LEN, 32, 4, 8, 32]) {
                *buffer = (worker.api.clCreateBuffer)(
                    worker.context,
                    1,
                    size,
                    ptr::null_mut(),
                    &mut status,
                );
                check(status, "buffer")?;
            }
        }
        worker.self_test()?;
        Ok(worker)
    }

    fn self_test(&self) -> Result<(), String> {
        let mut prefix = b"subtensor-pow-register-v1".to_vec();
        prefix.extend_from_slice(&1u16.to_le_bytes());
        prefix.extend_from_slice(&[1; 32]);
        prefix.extend_from_slice(&[2; 32]);
        prefix.extend_from_slice(&[3; 32]);
        for nonce in [0u64, 1, u64::MAX] {
            self.write(self.buffers[0], &prefix)?;
            // SAFETY: each argument is copied into the retained vector kernel.
            // The one-workitem kernel writes exactly 32 bytes to its output.
            unsafe {
                check(
                    (self.api.clSetKernelArg)(
                        self.vector_kernel,
                        0,
                        std::mem::size_of::<Handle>(),
                        (&self.buffers[0] as *const Handle).cast(),
                    ),
                    "vector prefix",
                )?;
                check(
                    (self.api.clSetKernelArg)(
                        self.vector_kernel,
                        1,
                        8,
                        (&nonce as *const u64).cast(),
                    ),
                    "vector nonce",
                )?;
                check(
                    (self.api.clSetKernelArg)(
                        self.vector_kernel,
                        2,
                        std::mem::size_of::<Handle>(),
                        (&self.buffers[4] as *const Handle).cast(),
                    ),
                    "vector output",
                )?;
                let global = 1usize;
                check(
                    (self.api.clEnqueueNDRangeKernel)(
                        self.queue,
                        self.vector_kernel,
                        1,
                        ptr::null(),
                        &global,
                        ptr::null(),
                        0,
                        ptr::null(),
                        ptr::null_mut(),
                    ),
                    "vector launch",
                )?;
            }
            let mut gpu = [0u8; 32];
            self.read(self.buffers[4], &mut gpu)?;
            if gpu != seal(&prefix, nonce)? {
                return Err(format!(
                    "OpenCL device {} failed registration hash self-test",
                    self.info.name
                ));
            }
        }
        Ok(())
    }

    fn write(&self, buffer: Handle, bytes: &[u8]) -> Result<(), String> {
        // SAFETY: buffers were allocated to fixed matching sizes, blocking write
        // completes before the byte slice can leave scope.
        unsafe {
            check(
                (self.api.clEnqueueWriteBuffer)(
                    self.queue,
                    buffer,
                    1,
                    0,
                    bytes.len(),
                    bytes.as_ptr().cast(),
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                ),
                "write",
            )
        }
    }
    fn read(&self, buffer: Handle, bytes: &mut [u8]) -> Result<(), String> {
        // SAFETY: blocking reads into live, correctly sized host buffers.
        unsafe {
            check(
                (self.api.clEnqueueReadBuffer)(
                    self.queue,
                    buffer,
                    1,
                    0,
                    bytes.len(),
                    bytes.as_mut_ptr().cast(),
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                ),
                "read",
            )
        }
    }
    fn arg<T>(&self, index: u32, value: &T) -> Result<(), String> {
        // SAFETY: OpenCL copies each argument's bytes before returning.
        unsafe {
            check(
                (self.api.clSetKernelArg)(
                    self.kernel,
                    index,
                    std::mem::size_of::<T>(),
                    (value as *const T).cast(),
                ),
                "kernel argument",
            )
        }
    }
    fn launch(
        &self,
        prefix: &[u8],
        limit: &[u8; 32],
        start: u64,
        attempts: u32,
    ) -> Result<(), String> {
        self.write(self.buffers[0], prefix)?;
        self.write(self.buffers[1], limit)?;
        self.write(self.buffers[2], &0u32.to_ne_bytes())?;
        self.write(self.buffers[3], &0u64.to_ne_bytes())?;
        self.arg(0, &self.buffers[0])?;
        self.arg(1, &self.buffers[1])?;
        self.arg(2, &start)?;
        self.arg(3, &attempts)?;
        self.arg(4, &self.buffers[2])?;
        self.arg(5, &self.buffers[3])?;
        let global = (attempts as usize).min(65_536);
        // SAFETY: kernel arguments and buffers have been initialized; the
        // runtime chooses a compatible local workgroup size.
        unsafe {
            check(
                (self.api.clEnqueueNDRangeKernel)(
                    self.queue,
                    self.kernel,
                    1,
                    ptr::null(),
                    &global,
                    ptr::null(),
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                ),
                "launch",
            )?;
            check((self.api.clFlush)(self.queue), "flush")
        }
    }
    fn answer(&self) -> Result<Option<u64>, String> {
        let mut found = [0u8; 4];
        self.read(self.buffers[2], &mut found)?;
        if u32::from_ne_bytes(found) == 0 {
            return Ok(None);
        }
        let mut nonce = [0u8; 8];
        self.read(self.buffers[3], &mut nonce)?;
        Ok(Some(u64::from_ne_bytes(nonce)))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // SAFETY: release only successfully created handles. Finishing the
        // queue ensures outstanding kernels no longer reference freed buffers.
        unsafe {
            if !self.queue.is_null() {
                (self.api.clFinish)(self.queue);
            }
            for buffer in self.buffers {
                if !buffer.is_null() {
                    (self.api.clReleaseMemObject)(buffer);
                }
            }
            if !self.vector_kernel.is_null() {
                (self.api.clReleaseKernel)(self.vector_kernel);
            }
            if !self.kernel.is_null() {
                (self.api.clReleaseKernel)(self.kernel);
            }
            if !self.program.is_null() {
                (self.api.clReleaseProgram)(self.program);
            }
            if !self.queue.is_null() {
                (self.api.clReleaseCommandQueue)(self.queue);
            }
            if !self.context.is_null() {
                (self.api.clReleaseContext)(self.context);
            }
        }
    }
}

/// Persistent per-device contexts. Every device receives a disjoint nonce
/// interval in each bounded batch, and launches are flushed before results wait.
pub struct GpuMiner {
    workers: Vec<Worker>,
}

impl GpuMiner {
    pub fn new(device_ids: Option<&[usize]>) -> Result<Self, String> {
        let api = Api::load()?;
        let devices = discover(&api)?;
        if devices.is_empty() {
            return Err("no usable OpenCL GPUs found; install the GPU vendor's OpenCL driver or select CPU mining".into());
        }
        if let Some(ids) = device_ids {
            if ids.is_empty() || ids.len() > 32 {
                return Err("select 1..32 GPU devices".into());
            }
            let mut unique = ids.to_vec();
            unique.sort_unstable();
            unique.dedup();
            if unique.len() != ids.len() || ids.iter().any(|id| *id >= devices.len()) {
                return Err("GPU device ids must be unique discovered device ids".into());
            }
        } else if devices.len() > 32 {
            return Err("more than 32 GPUs found; select at most 32".into());
        }
        let mut workers = Vec::new();
        for (device, info) in devices {
            if device_ids.is_none_or(|ids| ids.contains(&info.id)) {
                workers.push(Worker::new(Arc::clone(&api), device, info)?);
            }
        }
        Ok(Self { workers })
    }
    pub fn devices(&self) -> Vec<GpuDevice> {
        self.workers
            .iter()
            .map(|worker| worker.info.clone())
            .collect()
    }

    pub fn mine(
        &mut self,
        prefix: &[u8],
        difficulty: u64,
        start: u64,
        attempts_per_device: u32,
    ) -> Result<Option<Solution>, String> {
        let limit = validate_batch(prefix, difficulty, attempts_per_device)?;
        let bytes = limit.to_little_endian();
        for (index, worker) in self.workers.iter().enumerate() {
            let offset = (index as u64).wrapping_mul(u64::from(attempts_per_device));
            worker.launch(
                prefix,
                &bytes,
                start.wrapping_add(offset),
                attempts_per_device,
            )?;
        }
        let mut solution = None;
        // Drain every queue before returning, including when another GPU won.
        for (index, worker) in self.workers.iter().enumerate() {
            if let Some(nonce) = worker.answer()? {
                let device_start =
                    start.wrapping_add((index as u64).wrapping_mul(u64::from(attempts_per_device)));
                if nonce.wrapping_sub(device_start) >= u64::from(attempts_per_device) {
                    return Err("GPU returned a nonce outside its assigned range".into());
                }
                let work = seal(prefix, nonce)?;
                if U256::from_little_endian(&work) > limit {
                    return Err("GPU returned an invalid registration proof".into());
                }
                if solution.is_none() {
                    solution = Some((nonce, work));
                }
            }
        }
        Ok(solution)
    }
}
