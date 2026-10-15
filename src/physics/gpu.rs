//! Solver LBM na karcie graficznej (wgpu, shader obliczeniowy `lbm.wgsl`).
//!
//! Populacje żyją wyłącznie w pamięci GPU; na CPU wracają tylko wielkości makroskopowe (ρ, u, ν_t)
//! i suma wymiany pędu — raz na porcję kroków, gdy interfejs potrzebuje nowej klatki. Geometria
//! (rodzaje komórek, odległości ścian, strefy pochłaniające, profil wlotu) jest wysyłana tylko po
//! zmianie sceny. Gdy karta nie jest dostępna albo siatka się nie mieści, `Lattice` liczy na CPU.

use std::sync::{mpsc, Arc, OnceLock};

use wgpu::util::DeviceExt;

use super::lattice::Lattice;

/// Odstęp między parametrami kolejnych kroków w buforze (wymagane wyrównanie dynamicznego przesunięcia).
const SLOT: u64 = 256;
/// Ile kroków można zlecić w jednym przesłaniu do kolejki.
const MAX_SLOTS: usize = 256;
const WG: u32 = 128;

pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    limits: wgpu::Limits,
    /// Nazwa karty do pokazania w interfejsie.
    pub name: String,
}

static CONTEXT: OnceLock<Option<Arc<GpuContext>>> = OnceLock::new();

/// Wspólny kontekst GPU (tworzony przy pierwszym użyciu). `None`, gdy nie ma sprzętowej karty
/// obsługującej shadery obliczeniowe.
pub fn context() -> Option<Arc<GpuContext>> {
    CONTEXT
        .get_or_init(|| match GpuContext::new() {
            Ok(c) => {
                log::info!("obliczenia na GPU: {}", c.name);
                Some(Arc::new(c))
            }
            Err(e) => {
                log::warn!("GPU niedostępne, obliczenia na CPU: {e}");
                None
            }
        })
        .clone()
}

impl GpuContext {
    fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err(format!("tylko programowa karta ({})", info.name));
        }
        if !adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
            return Err(format!("{} nie obsługuje shaderów obliczeniowych", info.name));
        }
        let al = adapter.limits();
        let limits = wgpu::Limits {
            max_storage_buffers_per_shader_stage: 8,
            max_storage_buffer_binding_size: al.max_storage_buffer_binding_size,
            max_buffer_size: al.max_buffer_size,
            max_compute_workgroups_per_dimension: al.max_compute_workgroups_per_dimension,
            ..wgpu::Limits::downlevel_defaults()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("aeroflow-lbm"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| e.to_string())?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lbm"),
            source: wgpu::ShaderSource::Wgsl(include_str!("lbm.wgsl").into()),
        });
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let uniform = |binding: u32, dynamic: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: None },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lbm"),
            entries: &[
                uniform(0, false),
                uniform(1, true),
                storage(2, true),
                storage(3, false),
                storage(4, true),
                storage(5, false),
                storage(6, true),
                storage(7, true),
                storage(8, false),
                storage(9, false),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lbm"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("lbm"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("step"),
            compilation_options: Default::default(),
            cache: None,
        });
        let name = format!("{} ({:?})", info.name, info.backend);
        Ok(Self { device, queue, pipeline, layout, limits, name })
    }

    /// Czy siatka o `n` komórkach zmieści się w buforach tej karty.
    pub fn fits(&self, n: usize) -> bool {
        let f = 9 * n as u64 * 4;
        f <= self.limits.max_storage_buffer_binding_size as u64 && f <= self.limits.max_buffer_size
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    nx: u32,
    ny: u32,
    n: u32,
    nbodies: u32,
    tau0: f32,
    les_k: f32,
    u_wall: f32,
    keep_bulk: f32,
    top: u32,
    bottom: u32,
    groups_x: u32,
    turb_on: u32,
    turbulence: f32,
    u_lb: f32,
    win_h: f32,
    _pad0: f32,
    o_sponge_row: u32,
    o_sponge_col: u32,
    o_eq: u32,
    o_profile: u32,
    o_pivot: u32,
    o_phase: u32,
    o_linkq: u32,
    _pad1: u32,
}

/// Wartości z CPU potrzebne jądru w danym kroku.
pub struct StepInput {
    pub tau0: f32,
    pub les_k: f32,
    pub keep_bulk: f32,
    pub rho_ref: f32,
}

pub struct GpuSolver {
    ctx: Arc<GpuContext>,
    nx: usize,
    ny: usize,
    n: usize,
    f: [wgpu::Buffer; 2],
    mac: [wgpu::Buffer; 2],
    cell: wgpu::Buffer,
    aux: wgpu::Buffer,
    force: wgpu::Buffer,
    flags: wgpu::Buffer,
    params: wgpu::Buffer,
    steps_buf: wgpu::Buffer,
    readback: wgpu::Buffer,
    bind: Option<[wgpu::BindGroup; 2]>,
    /// Który z buforów `f`/`mac` zawiera bieżący stan.
    cur: usize,
    groups: (u32, u32),
    params_host: Params,
    /// Ciało, któremu odpowiada każda para (połączenie, kierunek); `u16::MAX` — brak.
    link_body: Vec<u16>,
    pending: usize,
    pub geometry_dirty: bool,
    pub state_dirty: bool,
}

fn storage_buf(device: &wgpu::Device, label: &str, size: u64, extra: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC | extra,
        mapped_at_creation: false,
    })
}

impl GpuSolver {
    pub fn new(ctx: Arc<GpuContext>, nx: usize, ny: usize) -> Option<Self> {
        let n = nx * ny;
        if !ctx.fits(n) {
            return None;
        }
        let d = &ctx.device;
        let fb = 9 * n as u64 * 4;
        let mb = 4 * n as u64 * 4;
        let f = [storage_buf(d, "f0", fb, wgpu::BufferUsages::empty()), storage_buf(d, "f1", fb, wgpu::BufferUsages::empty())];
        let mac = [storage_buf(d, "mac0", mb, wgpu::BufferUsages::empty()), storage_buf(d, "mac1", mb, wgpu::BufferUsages::empty())];
        let cell = storage_buf(d, "cell", 2 * n as u64 * 4, wgpu::BufferUsages::empty());
        let flags = storage_buf(d, "flags", 16, wgpu::BufferUsages::empty());
        let params = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let steps_buf = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("steps"),
            size: SLOT * MAX_SLOTS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let total = n.div_ceil(WG as usize) as u32;
        let maxg = ctx.limits.max_compute_workgroups_per_dimension.max(1);
        let gx = total.min(maxg);
        let gy = total.div_ceil(gx);
        let readback = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: mb + 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            nx,
            ny,
            n,
            f,
            mac,
            cell,
            aux: storage_buf(d, "aux", 16, wgpu::BufferUsages::empty()),
            force: storage_buf(d, "force", 16, wgpu::BufferUsages::empty()),
            flags,
            params,
            steps_buf,
            readback,
            bind: None,
            cur: 0,
            groups: (gx, gy),
            params_host: bytemuck::Zeroable::zeroed(),
            link_body: Vec::new(),
            pending: 0,
            geometry_dirty: true,
            state_dirty: true,
            ctx,
        })
    }

    pub fn name(&self) -> &str {
        &self.ctx.name
    }

    /// Wysyła geometrię i stałe pola (po zmianie ciał, profilu wlotu lub prędkości).
    pub fn upload_geometry(&mut self, lat: &Lattice) {
        let d = &self.ctx.device;
        let n = self.n;
        let mut cell = vec![0u32; 2 * n];
        for c in 0..n {
            cell[c] = lat.kind[c] as u32 | ((lat.labels[c] as u32) << 8);
            cell[n + c] = lat.link_idx[c];
        }
        self.ctx.queue.write_buffer(&self.cell, 0, bytemuck::cast_slice(&cell));

        let mut aux: Vec<f32> = Vec::new();
        let mut put = |v: &[f32]| {
            let o = aux.len() as u32;
            aux.extend_from_slice(v);
            o
        };
        let o_sponge_row = put(&lat.sponge_row);
        let o_sponge_col = put(&lat.sponge_col);
        let eq: Vec<f32> = lat.eq_row.iter().flatten().copied().collect();
        let o_eq = put(&eq);
        let o_profile = put(&lat.profile);
        let piv: Vec<f32> = lat.pivots.iter().flat_map(|p| [p[0] as f32, p[1] as f32]).collect();
        let o_pivot = put(&piv);
        let ph: Vec<f32> = lat.phases.iter().flatten().copied().collect();
        let o_phase = put(&ph);
        let lq: Vec<f32> = lat.link_q.iter().flatten().copied().collect();
        let o_linkq = put(&lq);
        self.aux = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aux"),
            contents: bytemuck::cast_slice(&aux),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        // dla każdego połączenia i kierunku: które ciało dostaje pęd (sąsiad pod −c_i)
        let nl = lat.link_q.len();
        let mut link_body = vec![u16::MAX; nl * 9];
        let (nx, ny) = (self.nx as i32, self.ny as i32);
        for c in 0..n {
            let li = lat.link_idx[c];
            if li == u32::MAX {
                continue;
            }
            let (x, y) = ((c % self.nx) as i32, (c / self.nx) as i32);
            for i in 0..9 {
                let sx = x - super::lattice::CX[i];
                let sy = y - super::lattice::CY[i];
                if sx < 0 || sy < 0 || sx >= nx || sy >= ny {
                    continue;
                }
                let lab = lat.labels[(sy * nx + sx) as usize];
                if lab != 0 {
                    link_body[li as usize * 9 + i] = lab - 1;
                }
            }
        }
        self.link_body = link_body;
        self.force = storage_buf(d, "force", (nl * 27 * 4) as u64, wgpu::BufferUsages::empty());
        if self.readback.size() < (4 * n as u64 * 4) + 16 + (nl * 27 * 4) as u64 {
            self.readback = d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: (4 * n as u64 * 4) + 16 + (nl * 27 * 4) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        let boundary = |b: super::lattice::Boundary| match b {
            super::lattice::Boundary::FreeStream => 0,
            super::lattice::Boundary::SlipWall => 1,
            super::lattice::Boundary::NoSlipWall => 2,
            super::lattice::Boundary::MovingGround => 3,
        };
        self.params_host = Params {
            nx: self.nx as u32,
            ny: self.ny as u32,
            n: n as u32,
            nbodies: lat.pivots.len() as u32,
            tau0: 0.0,
            les_k: 0.0,
            u_wall: lat.u_lb,
            keep_bulk: 0.0,
            top: boundary(lat.layout.top),
            bottom: boundary(lat.layout.bottom),
            groups_x: self.groups.0,
            turb_on: 0,
            turbulence: 0.0,
            u_lb: lat.u_lb,
            win_h: lat.win.h as f32,
            _pad0: 0.0,
            o_sponge_row,
            o_sponge_col,
            o_eq,
            o_profile,
            o_pivot,
            o_phase,
            o_linkq,
            _pad1: 0,
        };
        self.rebuild_bind_groups();
        self.geometry_dirty = false;
    }

    /// Wysyła stan przepływu (populacje i wielkości makroskopowe) — po resecie.
    pub fn upload_state(&mut self, lat: &Lattice) {
        let q = &self.ctx.queue;
        q.write_buffer(&self.f[self.cur], 0, bytemuck::cast_slice(&lat.f));
        let n = self.n as u64 * 4;
        let m = &self.mac[self.cur];
        q.write_buffer(m, 0, bytemuck::cast_slice(&lat.rho));
        q.write_buffer(m, n, bytemuck::cast_slice(&lat.ux));
        q.write_buffer(m, 2 * n, bytemuck::cast_slice(&lat.uy));
        q.write_buffer(m, 3 * n, bytemuck::cast_slice(&lat.nut));
        self.state_dirty = false;
    }

    fn rebuild_bind_groups(&mut self) {
        let d = &self.ctx.device;
        let mk = |a: usize| {
            let b = 1 - a;
            d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lbm"),
                layout: &self.ctx.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.params.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.steps_buf,
                            offset: 0,
                            size: wgpu::BufferSize::new(16),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 2, resource: self.f[a].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: self.f[b].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: self.mac[a].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: self.mac[b].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: self.cell.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: self.aux.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 8, resource: self.force.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 9, resource: self.flags.as_entire_binding() },
                ],
            })
        };
        self.bind = Some([mk(0), mk(1)]);
    }

    /// Zleca `steps` kroków (pierwszy ma numer `first_step`) i czeka na ich wykonanie — dzięki temu
    /// pomiar czasu w wątku symulacji odpowiada pracy karty, a kolejka nie rośnie bez końca.
    pub fn run(&mut self, lat: &Lattice, first_step: u64, steps: usize, inp: &StepInput) {
        if steps == 0 {
            return;
        }
        let mut p = self.params_host;
        p.tau0 = inp.tau0;
        p.les_k = inp.les_k;
        p.keep_bulk = inp.keep_bulk;
        p.u_wall = lat.u_lb;
        p.u_lb = lat.u_lb;
        p.turbulence = lat.turbulence;
        p.turb_on = (lat.turbulence > 0.0) as u32;
        let q = &self.ctx.queue;
        q.write_buffer(&self.params, 0, bytemuck::bytes_of(&p));
        if self.pending == 0 {
            // nowa porcja: zerowanie sum wymiany pędu i flagi niestabilności
            let mut enc = self.ctx.device.create_command_encoder(&Default::default());
            enc.clear_buffer(&self.force, 0, None);
            enc.clear_buffer(&self.flags, 0, None);
            q.submit([enc.finish()]);
        }
        let mut done = 0;
        let mut last = None;
        while done < steps {
            let k = (steps - done).min(MAX_SLOTS);
            let mut slots = vec![0u8; SLOT as usize * k];
            for j in 0..k {
                let s = (first_step + (done + j) as u64) as u32;
                slots[j * SLOT as usize..j * SLOT as usize + 4].copy_from_slice(&s.to_le_bytes());
                slots[j * SLOT as usize + 4..j * SLOT as usize + 8].copy_from_slice(&inp.rho_ref.to_le_bytes());
            }
            q.write_buffer(&self.steps_buf, 0, &slots);
            let mut enc = self.ctx.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.ctx.pipeline);
                let bind = self.bind.as_ref().expect("geometria GPU nie została wysłana");
                for j in 0..k {
                    pass.set_bind_group(0, &bind[self.cur], &[(j as u64 * SLOT) as u32]);
                    pass.dispatch_workgroups(self.groups.0, self.groups.1, 1);
                    self.cur = 1 - self.cur;
                }
            }
            last = Some(q.submit([enc.finish()]));
            done += k;
        }
        self.pending += steps;
        if let Some(idx) = last {
            let _ = self.ctx.device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None });
        }
    }

    /// Porzuca sumy sił niedokończonej porcji (po resecie albo zmianie geometrii).
    pub fn discard_pending(&mut self) {
        self.pending = 0;
    }

    /// Kopiuje bieżące pola do `lat` i zwraca sumę wymiany pędu dla każdego ciała, liczbę kroków
    /// porcji i informację o niestabilności.
    pub fn read(&mut self, lat: &mut Lattice) -> Result<(Vec<[f64; 3]>, usize, bool), String> {
        let n = self.n;
        let mb = 4 * n as u64 * 4;
        let nl = self.link_body.len() / 9;
        let fb = (nl * 27 * 4) as u64;
        let mut enc = self.ctx.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.mac[self.cur], 0, &self.readback, 0, mb);
        enc.copy_buffer_to_buffer(&self.flags, 0, &self.readback, mb, 16);
        if fb > 0 {
            enc.copy_buffer_to_buffer(&self.force, 0, &self.readback, mb + 16, fb);
        }
        self.ctx.queue.submit([enc.finish()]);
        let slice = self.readback.slice(0..mb + 16 + fb);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.ctx.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        let nb = lat.pivots.len();
        let mut forces = vec![[0f64; 3]; nb];
        let bad;
        {
            let data = slice.get_mapped_range();
            let all: &[f32] = bytemuck::cast_slice(&data);
            lat.rho.copy_from_slice(&all[..n]);
            lat.ux.copy_from_slice(&all[n..2 * n]);
            lat.uy.copy_from_slice(&all[2 * n..3 * n]);
            lat.nut.copy_from_slice(&all[3 * n..4 * n]);
            let fl: &[u32] = bytemuck::cast_slice(&data[mb as usize..mb as usize + 4]);
            bad = fl[0] != 0;
            let fr = &all[4 * n + 4..4 * n + 4 + nl * 27];
            for (j, &b) in self.link_body.iter().enumerate() {
                if (b as usize) < nb {
                    for c in 0..3 {
                        forces[b as usize][c] += fr[j * 3 + c] as f64;
                    }
                }
            }
        }
        self.readback.unmap();
        let steps = std::mem::take(&mut self.pending);
        Ok((forces, steps, bad))
    }
}

#[cfg(test)]
mod tests {
    use super::super::geometry::{Body, Naca, Shape};
    use super::super::lattice::{Boundary, InletProfile, Lattice, Layout};

    fn scene(top: Boundary, bottom: Boundary) -> Lattice {
        let mut lat = Lattice::new(Layout { nx: 240, ny: 120, top, bottom });
        lat.set_params(0.06, 0.002, 0.15, 0.02, InletProfile::Uniform);
        let bodies = [
            Body::new(1, Shape::Circle { r: 8.0 }, [60.0, 64.0], 0.0),
            Body::new(2, Shape::Airfoil { chord: 40.0, naca: Naca::new(0.02, 0.4, 0.12) }, [140.0, 50.0], 8.0),
        ];
        lat.set_bodies(&bodies, 1.0);
        lat.reset_flow();
        lat
    }

    /// GPU liczy to samo co CPU: pola i siły po 600 krokach zgodne z dokładnością arytmetyki f32.
    #[test]
    fn gpu_matches_cpu() {
        if super::context().is_none() {
            eprintln!("brak GPU — test pominięty");
            return;
        }
        for (top, bottom) in [
            (Boundary::FreeStream, Boundary::FreeStream),
            (Boundary::SlipWall, Boundary::MovingGround),
            (Boundary::NoSlipWall, Boundary::NoSlipWall),
        ] {
            let mut cpu = scene(top, bottom);
            let mut gpu = scene(top, bottom);
            assert!(gpu.enable_gpu().is_some());
            for _ in 0..6 {
                cpu.step(100).unwrap();
                gpu.advance(60).unwrap();
                gpu.advance(40).unwrap();
                gpu.sync().unwrap();
            }
            let mut du = 0f32;
            for c in 0..cpu.cell_count() {
                du = du.max((cpu.ux[c] - gpu.ux[c]).abs()).max((cpu.uy[c] - gpu.uy[c]).abs()).max((cpu.rho[c] - gpu.rho[c]).abs());
            }
            assert!(du < 2e-4, "{top:?}/{bottom:?}: największa różnica pól {du}");
            for (a, b) in cpu.forces.iter().zip(&gpu.forces) {
                for k in 0..3 {
                    let scale = a[k].abs().max(1e-3);
                    assert!((a[k] - b[k]).abs() / scale < 5e-3, "{top:?}/{bottom:?}: siła {k}: CPU {} GPU {}", a[k], b[k]);
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn gpu_mlups() {
        if super::context().is_none() {
            return;
        }
        let mut lat = Lattice::new(Layout { nx: 1200, ny: 600, top: Boundary::FreeStream, bottom: Boundary::FreeStream });
        lat.set_params(0.06, 0.0001, 0.15, 0.005, InletProfile::Uniform);
        lat.set_bodies(&[Body::new(1, Shape::Circle { r: 40.0 }, [300.0, 300.0], 0.0)], 1.0);
        println!("{}", lat.enable_gpu().unwrap());
        lat.step(100).unwrap();
        let t = std::time::Instant::now();
        let n = 2000;
        lat.advance(n).unwrap();
        lat.sync().unwrap();
        let dt = t.elapsed().as_secs_f64();
        println!("cells {} GPU MLUPS {:.0}", lat.cell_count(), (lat.cell_count() * n) as f64 / dt / 1e6);
    }
}
