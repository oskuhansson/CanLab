//! W7.5: what this machine's adapter is, and how to run a kernel on it.
//!
//! This is the only crate in the workspace that depends on a graphics stack.
//! Everything else is pure and testable without a driver; this one is the
//! boundary, and it exists so that the rest of the tree can be checked without
//! one.
//!
//! # What the probe found on this machine
//!
//! One adapter, `Vulkan llvmpipe (LLVM 21.1.8, 256 bits)`, device type `Cpu` —
//! the software Vulkan implementation, not a discrete GPU. It reports
//! [`wgpu::Features::SHADER_F64`], so the double-precision path the roadmap's
//! W7.5 asks about is available *here*.
//!
//! That is a real answer to the roadmap's question and it is also a warning
//! about what it means: llvmpipe executes on the CPU, so a throughput measured
//! on it is not a GPU's throughput. The roadmap's 10⁷ photons per second stays
//! unmeasured, and the doc says so rather than quoting a number from a software
//! rasteriser.
//!
//! # What running a kernel means here
//!
//! [`run`] takes the WGSL [`canlab_core::wgsl::compile`] produced and executes it
//! once per input row, returning one `f32` per row. That is the GPU half of the
//! differential test W7.2 asks for: the same program through
//! [`canlab_core::wgsl::interpret`] and through the adapter, compared.

use canlab_core::wgsl::Kernel;

/// What an adapter is, in the terms the roadmap asks about.
#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    /// The backend's name, e.g. `Vulkan`.
    pub backend: String,
    /// The device's name.
    pub name: String,
    /// Whether it is a CPU, an integrated GPU or a discrete one.
    pub device_type: String,
    /// Whether it offers `f64` in a shader.
    pub shader_f64: bool,
}

impl Adapter {
    /// A line a probe binary prints.
    pub fn describe(&self) -> String {
        format!(
            "{} {} ({}) — SHADER_F64: {}",
            self.backend, self.name, self.device_type, self.shader_f64
        )
    }
}

/// Why a kernel could not be run.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuError {
    /// No adapter at all.
    NoAdapter,
    /// The adapter refused a device.
    NoDevice(String),
    /// The shader did not compile.
    Shader(String),
    /// The kernel takes no parameters, so there is nothing to vary.
    NoParameters,
    /// A row of inputs has the wrong length.
    RowLength {
        /// What the kernel takes.
        expected: usize,
        /// What the row has.
        got: usize,
    },
}

impl GpuError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            GpuError::NoAdapter => "no adapter is available on this machine".to_string(),
            GpuError::NoDevice(why) => format!("the adapter refused a device: {why}"),
            GpuError::Shader(why) => format!("the shader did not compile: {why}"),
            GpuError::NoParameters => {
                "the kernel takes no parameters, so there is nothing to vary".to_string()
            }
            GpuError::RowLength { expected, got } => {
                format!("the kernel takes {expected} parameters and a row has {got}")
            }
        }
    }
}

/// Serialises every GPU call in this process.
///
/// Two threads creating a Vulkan instance and a device at the same time on
/// llvmpipe **segfault** — measured: the differential test's two cases run in
/// parallel by default and the process died with `SIGSEGV` before either printed
/// anything, while `--test-threads=1` passed. A driver that is not thread-safe is
/// not something a library can fix, so the library serialises instead: one call
/// at a time, which costs nothing for a probe and little for a test.
static GPU_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The instance, with every backend this build has.
fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        flags: wgpu::InstanceFlags::empty(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::default(),
        display: None,
    })
}

/// Every adapter this machine offers, with the one feature W7.5 asks about.
pub fn probe() -> Vec<Adapter> {
    let _guard = GPU_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let instance = instance();
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    adapters
        .iter()
        .map(|adapter| {
            let info = adapter.get_info();
            Adapter {
                backend: format!("{:?}", info.backend),
                name: info.name,
                device_type: format!("{:?}", info.device_type),
                shader_f64: adapter
                    .features()
                    .contains(wgpu::Features::SHADER_F64),
            }
        })
        .collect()
}

/// The first adapter, or `None`.
fn first_adapter() -> Option<wgpu::Adapter> {
    let instance = instance();
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .into_iter()
        .next()
}

/// The whole shader: the kernel, a `rand` for it to call, and an entry point.
///
/// The kernel's own source is used unchanged, which is the point — a kernel that
/// had to be rewritten for the GPU would be a second source.
fn shader(kernel: &Kernel, parameters: usize) -> String {
    let arguments: Vec<String> = (0..parameters)
        .map(|index| format!("inputs[base + {index}]"))
        .collect();
    format!(
        "{}\n\
         // The random stream, in the same u32 xorshift the interpreter uses. The\n\
         // state is a private global seeded from the invocation index, so each\n\
         // photon has its own stream and every call advances it.\n\
         var<private> rng: u32 = 1u;\n\
         fn rand() -> f32 {{\n\
         \x20 rng = rng ^ (rng << 13u);\n\
         \x20 rng = rng ^ (rng >> 17u);\n\
         \x20 rng = rng ^ (rng << 5u);\n\
         \x20 return f32(rng >> 8u) / 16777216.0;\n\
         }}\n\
         @group(0) @binding(0) var<storage, read> inputs: array<f32>;\n\
         @group(0) @binding(1) var<storage, read_write> outputs: array<f32>;\n\
         @compute @workgroup_size(64)\n\
         fn main(@builtin(global_invocation_id) id: vec3<u32>) {{\n\
         \x20 let i = id.x;\n\
         \x20 if (i >= arrayLength(&outputs)) {{ return; }}\n\
         \x20 let base = i * {parameters}u;\n\
         \x20 rng = (i * 0x9E3779B9u) | 1u;\n\
         \x20 outputs[i] = kernel({});\n\
         }}\n",
        kernel.wgsl,
        arguments.join(", ")
    )
}

/// Run a compiled kernel once per row of `inputs`, on the first adapter.
///
/// Each row supplies the kernel's parameters in the order
/// [`Kernel::parameters`] lists them.
pub fn run(kernel: &Kernel, inputs: &[Vec<f32>]) -> Result<Vec<f32>, GpuError> {
    Session::new()?.run(kernel, inputs)
}

/// A device, kept open across many kernels.
///
/// Creating a device costs more than running a small kernel: the 512-point
/// differential test spends most of its 1.4 s in `request_device`. A sweep over
/// two hundred programs would pay that two hundred times, so the device is a
/// value the caller can hold.
pub struct Session {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Session {
    /// Open the first adapter and a device on it.
    pub fn new() -> Result<Session, GpuError> {
        let _guard = GPU_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let adapter = first_adapter().ok_or(GpuError::NoAdapter)?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("canlab-gpu"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            }))
            .map_err(|error| GpuError::NoDevice(format!("{error:?}")))?;
        Ok(Session { device, queue })
    }

    /// Run a compiled kernel once per row of `inputs`.
    pub fn run(&self, kernel: &Kernel, inputs: &[Vec<f32>]) -> Result<Vec<f32>, GpuError> {
        let _guard = GPU_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        self.dispatch(kernel, inputs)
    }

    fn dispatch(&self, kernel: &Kernel, inputs: &[Vec<f32>]) -> Result<Vec<f32>, GpuError> {
    let parameters = kernel.parameters.len();
    if parameters == 0 {
        return Err(GpuError::NoParameters);
    }
    for row in inputs {
        if row.len() != parameters {
            return Err(GpuError::RowLength {
                expected: parameters,
                got: row.len(),
            });
        }
    }
    let device = &self.device;
    let queue = &self.queue;
    let source = shader(kernel, parameters);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("kernel"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });

    let flat: Vec<f32> = inputs.iter().flatten().copied().collect();
    let count = inputs.len() as u64;
    let input_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("inputs"),
        size: (flat.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&input_buffer, 0, bytemuck_cast(&flat));
    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("outputs"),
        size: (count * 4).max(4),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (count * 4).max(4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bind group"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output_buffer.as_entire_binding(),
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let workgroups = (count.div_ceil(64)) as u32;
        pass.dispatch_workgroups(workgroups.max(1), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output_buffer, 0, &readback, 0, (count * 4).max(4));
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    })
    .ok();
    receiver
        .recv()
        .map_err(|_| GpuError::NoDevice(String::from("the readback was cancelled")))?
        .map_err(|error| GpuError::NoDevice(format!("the readback failed: {error:?}")))?;
    let data = slice
        .get_mapped_range()
        .map_err(|error| GpuError::NoDevice(format!("the readback could not be read: {error:?}")))?;
    let values: Vec<f32> = data
        .chunks_exact(4)
        .take(count as usize)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    drop(data);
    readback.unmap();
    Ok(values)
    }
}

/// W7.5's accuracy check: how close a shader's `f64` gets to the CPU's.
///
/// The roadmap asks for "a 1e-15 accuracy check". This runs a hand-written `f64`
/// shader that computes `sqrt(2)` by Newton's method and a sum of thirds, and
/// returns the largest relative difference from the same computation in Rust.
///
/// `None` when the adapter does not offer `SHADER_F64`, which is the decision
/// W7.5 exists to make: without it the transport has to run in single precision
/// with a visible marker, or in double-double.
pub fn f64_accuracy() -> Result<Option<f64>, GpuError> {
    let _guard = GPU_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let adapter = first_adapter().ok_or(GpuError::NoAdapter)?;
    if !adapter.features().contains(wgpu::Features::SHADER_F64) {
        return Ok(None);
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("canlab-gpu-f64"),
        required_features: wgpu::Features::SHADER_F64,
        required_limits: wgpu::Limits::downlevel_defaults(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|error| GpuError::NoDevice(format!("{error:?}")))?;

    // Newton on `x² − 2` from 1.5, ten times: `f64` converges to `sqrt(2)` to
    // the last bit, `f32` would stop at `1e-7`.
    let source = r#"
@group(0) @binding(0) var<storage, read_write> out: array<f64, 2>;
@compute @workgroup_size(1)
fn main() {
  var x: f64 = 1.5;
  for (var i = 0; i < 10; i = i + 1) {
    x = x - (x * x - 2.0) / (2.0 * x);
  }
  out[0] = x;
  var s: f64 = 0.0;
  for (var i = 0; i < 100; i = i + 1) {
    s = s + 1.0 / 3.0;
  }
  out[1] = s;
}
"#;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("accuracy"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("out"),
        size: 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bind group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&buffer, 0, &readback, 0, 16);
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    })
    .ok();
    receiver
        .recv()
        .map_err(|_| GpuError::NoDevice(String::from("the readback was cancelled")))?
        .map_err(|error| GpuError::NoDevice(format!("the readback failed: {error:?}")))?;
    let data = slice
        .get_mapped_range()
        .map_err(|error| GpuError::NoDevice(format!("the readback could not be read: {error:?}")))?;
    let root = f64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
    let sum = f64::from_le_bytes([data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15]]);
    drop(data);
    readback.unmap();

    let expected_root = 2.0f64.sqrt();
    let expected_sum = (0..100).map(|_| 1.0f64 / 3.0).sum::<f64>();
    let root_error = ((root - expected_root) / expected_root).abs();
    let sum_error = ((sum - expected_sum) / expected_sum).abs();
    Ok(Some(root_error.max(sum_error)))
}

/// `&[f32]` as bytes, without a `bytemuck` dependency for one call.
fn bytemuck_cast(values: &[f32]) -> &[u8] {
    // SAFETY: `f32` has no padding and the slice is exactly its own bytes; the
    // lifetime is the caller's and the buffer is written before it is used.
    unsafe {
        std::slice::from_raw_parts(
            values.as_ptr() as *const u8,
            std::mem::size_of_val(values),
        )
    }
}
