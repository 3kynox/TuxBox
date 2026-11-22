//! Save the guest bindings displaced by an internal compute pass. Graphics
//! root arguments other than tables and IA/OM/RS state survive the pass.
use std::collections::BTreeMap;
use windows::Win32::Graphics::Direct3D12::*;

#[derive(Default)]
pub(super) struct Bindings {
    pub(super) pipeline: Option<Pipeline>,
    heaps: Vec<Option<ID3D12DescriptorHeap>>,
    compute: Option<ID3D12RootSignature>,
    graphics: Option<ID3D12RootSignature>,
    compute_args: BTreeMap<u32, Argument>,
    graphics_tables: BTreeMap<u32, D3D12_GPU_DESCRIPTOR_HANDLE>,
}

pub(super) enum Pipeline {
    Regular(ID3D12PipelineState),
    Ray(ID3D12StateObject),
}

pub(super) enum Argument {
    Table(D3D12_GPU_DESCRIPTOR_HANDLE),
    Srv(u64),
    Uav(u64),
    Constants(BTreeMap<u32, u32>),
}

impl Bindings {
    pub(super) fn new(initial: Option<&ID3D12PipelineState>) -> Self {
        Self {
            pipeline: initial.cloned().map(Pipeline::Regular),
            ..Default::default()
        }
    }

    pub(super) fn compute_root(&mut self, root: Option<&ID3D12RootSignature>) {
        if self.compute.as_ref() != root {
            self.compute_args.clear();
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

    pub(super) unsafe fn restore(&self, host: &ID3D12GraphicsCommandList4) {
        unsafe {
            host.SetDescriptorHeaps(&self.heaps);
            host.SetComputeRootSignature(self.compute.as_ref());
            for (&slot, arg) in &self.compute_args {
                match arg {
                    Argument::Table(base) => host.SetComputeRootDescriptorTable(slot, *base),
                    Argument::Srv(va) => host.SetComputeRootShaderResourceView(slot, *va),
                    Argument::Uav(va) => host.SetComputeRootUnorderedAccessView(slot, *va),
                    Argument::Constants(words) => {
                        for (&index, &value) in words {
                            host.SetComputeRoot32BitConstant(slot, value, index);
                        }
                    }
                }
            }
            for (&slot, &base) in &self.graphics_tables {
                host.SetGraphicsRootDescriptorTable(slot, base);
            }
            match self.pipeline.as_ref() {
                Some(Pipeline::Regular(pso)) => host.SetPipelineState(pso),
                Some(Pipeline::Ray(pso)) => host.SetPipelineState1(pso),
                None => {}
            }
        }
    }
}
