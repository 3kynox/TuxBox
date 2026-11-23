//! Save the guest bindings displaced by an internal compute pass. Graphics
//! root arguments other than tables and IA/OM/RS state survive the pass.
use crate::device::compute::{Pipeline as GuestPipeline, RootSource, SceneRoot, SceneSource};
use std::collections::BTreeMap;
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{IUnknown, Result};

#[derive(Default)]
pub(super) struct Bindings {
    pub(super) pipeline: Option<Pipeline>,
    heaps: Vec<Option<ID3D12DescriptorHeap>>,
    compute: Option<ID3D12RootSignature>,
    layout: Option<RootSource>,
    graphics: Option<ID3D12RootSignature>,
    compute_args: BTreeMap<u32, Argument>,
    graphics_tables: BTreeMap<u32, D3D12_GPU_DESCRIPTOR_HANDLE>,
}

// A private root descriptor captures its value when recording. A guest
// descriptor table may change until submission, especially under root v1.0.
#[derive(Clone, Copy, PartialEq)]
enum SceneLocation {
    Table(D3D12_CPU_DESCRIPTOR_HANDLE),
    Address(u64),
}

impl SceneLocation {
    unsafe fn resolve(self, owner: &IUnknown) -> Result<(ID3D12Resource, u64)> {
        unsafe {
            match self {
                Self::Table(cpu) => crate::device::blas::inline_scene(owner, cpu),
                Self::Address(va) => crate::device::blas::inline_scene_address(owner, va),
            }
        }
    }
}

#[derive(PartialEq)]
pub(super) struct SceneCheck {
    location: SceneLocation,
    pub(super) scene: (ID3D12Resource, u64),
    pub(super) geometry: Option<[(ID3D12Resource, u64); 2]>,
}

impl SceneCheck {
    pub(super) fn recheck_on_submit(&self) -> bool {
        matches!(self.location, SceneLocation::Table(_)) || self.geometry.is_some()
    }

    pub(super) fn validate(&self, list: &super::List) -> Result<()> {
        let owner = list.parent.as_ref().ok_or(E_INVALIDARG)?;
        if unsafe { self.location.resolve(owner)? } != self.scene {
            return Err(E_INVALIDARG.into());
        }
        if let Some(geometry) = &self.geometry
            && unsafe { crate::device::blas::inline_geometry(owner, self.scene.1)? } != *geometry
        {
            return Err(E_INVALIDARG.into());
        }
        Ok(())
    }
}

pub(super) enum Pipeline {
    Regular(ID3D12PipelineState, Option<SceneRoot>),
    Ray(ID3D12StateObject),
}

pub(super) enum Argument {
    Table(D3D12_GPU_DESCRIPTOR_HANDLE),
    Cbv(u64),
    Srv(u64),
    Uav(u64),
    Constants(BTreeMap<u32, u32>),
}

impl From<&GuestPipeline> for Pipeline {
    fn from(p: &GuestPipeline) -> Self {
        Self::Regular(p.native.clone(), p.state.scene_root.clone())
    }
}

impl Bindings {
    pub(super) fn new(initial: Option<&GuestPipeline>) -> Self {
        Self {
            pipeline: initial.map(Pipeline::from),
            ..Default::default()
        }
    }

    pub(super) fn compute_root(
        &mut self,
        root: Option<&ID3D12RootSignature>,
        layout: Option<&RootSource>,
    ) {
        if self.compute.as_ref() != root {
            self.compute_args.clear();
            self.layout = layout.cloned();
            self.compute = root.cloned();
        }
    }

    pub(super) fn graphics_root(&mut self, root: Option<&ID3D12RootSignature>) {
        if self.graphics.as_ref() != root {
            self.graphics_tables.clear();
            self.graphics = root.cloned();
        }
    }

    pub(super) fn heaps(&mut self, heaps: &[Option<ID3D12DescriptorHeap>]) {
        if self.heaps.len() != heaps.len() || heaps.iter().any(|heap| !self.heaps.contains(heap)) {
            self.compute_args
                .retain(|_, arg| !matches!(arg, Argument::Table(_)));
            self.graphics_tables.clear();
            self.heaps.clear();
            self.heaps.extend_from_slice(heaps);
        }
    }

    pub(super) fn table<const COMPUTE: bool>(
        &mut self,
        slot: u32,
        base: D3D12_GPU_DESCRIPTOR_HANDLE,
    ) {
        if COMPUTE {
            self.arg(slot, Argument::Table(base));
        } else {
            self.graphics_tables.insert(slot, base);
        }
    }

    pub(super) fn arg(&mut self, slot: u32, arg: Argument) {
        self.compute_args.insert(slot, arg);
    }

    pub(super) fn constants(&mut self, slot: u32, offset: u32, words: &[u32]) {
        let arg = self
            .compute_args
            .entry(slot)
            .or_insert_with(|| Argument::Constants(Default::default()));
        if !matches!(arg, Argument::Constants(_)) {
            *arg = Argument::Constants(Default::default());
        }
        if let Argument::Constants(values) = arg {
            values.extend((offset..).zip(words.iter().copied()));
        }
    }

    unsafe fn apply_compute_args(&self, host: &ID3D12GraphicsCommandList) {
        unsafe {
            for (&slot, arg) in &self.compute_args {
                match arg {
                    Argument::Table(base) => host.SetComputeRootDescriptorTable(slot, *base),
                    Argument::Cbv(va) => host.SetComputeRootConstantBufferView(slot, *va),
                    Argument::Srv(va) => host.SetComputeRootShaderResourceView(slot, *va),
                    Argument::Uav(va) => host.SetComputeRootUnorderedAccessView(slot, *va),
                    Argument::Constants(words) => {
                        for (&index, &value) in words {
                            host.SetComputeRoot32BitConstant(slot, value, index);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn scene_root(&self) -> Option<&SceneRoot> {
        match self.pipeline.as_ref() {
            Some(Pipeline::Regular(_, root)) => root.as_ref(),
            _ => None,
        }
    }

    pub(super) fn scene_resource(&self, list: &super::List) -> Result<SceneCheck> {
        let scene = self.scene_root().ok_or(E_INVALIDARG)?;
        if !self
            .layout
            .as_ref()
            .is_some_and(|layout| layout.matches(&scene.original))
        {
            return Err(E_INVALIDARG.into());
        }
        let owner = list.parent.as_ref().ok_or(E_INVALIDARG)?;
        let location = match scene.source {
            SceneSource::Table { slot, offset } => {
                SceneLocation::Table(self.table_entry(slot, offset)?)
            }
            SceneSource::RootSrv(slot) => {
                let Some(Argument::Srv(address)) = self.compute_args.get(&slot) else {
                    return Err(E_INVALIDARG.into());
                };
                SceneLocation::Address(*address)
            }
        };
        let backing = unsafe { location.resolve(owner)? };
        let geometry = if scene.program.triangle_geometry {
            Some(unsafe { crate::device::blas::inline_geometry(owner, backing.1)? })
        } else {
            None
        };
        Ok(SceneCheck {
            location,
            scene: backing,
            geometry,
        })
    }

    fn table_entry(&self, slot: u32, table_offset: u32) -> Result<D3D12_CPU_DESCRIPTOR_HANDLE> {
        let Some(Argument::Table(base)) = self.compute_args.get(&slot) else {
            return Err(E_INVALIDARG.into());
        };
        let heap = self
            .heaps
            .iter()
            .flatten()
            .find(|heap| unsafe { heap.GetDesc().Type == D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV })
            .ok_or(E_INVALIDARG)?;
        let mut native = None::<ID3D12Device>;
        unsafe { heap.GetDevice(&mut native)? };
        let stride = u64::from(unsafe {
            native
                .ok_or(E_INVALIDARG)?
                .GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
        });
        let start = unsafe { heap.GetGPUDescriptorHandleForHeapStart().ptr };
        let offset = base.ptr.checked_sub(start).ok_or(E_INVALIDARG)?;
        if stride == 0 || !offset.is_multiple_of(stride) {
            return Err(E_INVALIDARG.into());
        }
        let slot = (offset / stride)
            .checked_add(u64::from(table_offset))
            .ok_or(E_INVALIDARG)?;
        if slot >= u64::from(unsafe { heap.GetDesc().NumDescriptors }) {
            return Err(E_INVALIDARG.into());
        }
        let cpu_start = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
        let offset = slot
            .checked_mul(stride)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(E_INVALIDARG)?;
        let ptr = cpu_start.ptr.checked_add(offset).ok_or(E_INVALIDARG)?;
        Ok(D3D12_CPU_DESCRIPTOR_HANDLE { ptr })
    }

    pub(super) unsafe fn dispatch_scene(
        &self,
        host: &ID3D12GraphicsCommandList,
        root: &SceneRoot,
        va: u64,
        geometry: Option<[u64; 2]>,
        dispatch: impl FnOnce(),
    ) {
        unsafe {
            host.SetComputeRootSignature(&root.native);
            self.apply_compute_args(host);
            host.SetComputeRootShaderResourceView(root.appended, va);
            if let Some([instances, normals]) = geometry {
                host.SetComputeRootShaderResourceView(root.appended + 1, instances);
                host.SetComputeRootShaderResourceView(root.appended + 2, normals);
            }
            dispatch();
            host.SetComputeRootSignature(self.compute.as_ref());
            self.apply_compute_args(host);
        }
    }

    pub(super) unsafe fn restore(&self, host: &ID3D12GraphicsCommandList4) {
        unsafe {
            host.SetDescriptorHeaps(&self.heaps);
            host.SetComputeRootSignature(self.compute.as_ref());
            self.apply_compute_args(host);
            for (&slot, &base) in &self.graphics_tables {
                host.SetGraphicsRootDescriptorTable(slot, base);
            }
            match self.pipeline.as_ref() {
                Some(Pipeline::Regular(pso, _)) => host.SetPipelineState(pso),
                Some(Pipeline::Ray(pso)) => host.SetPipelineState1(pso),
                None => {}
            }
        }
    }
}
