use super::{List, parent, recording};
use crate::device::compute::root_signature;
use core::ffi::c_void;
use windows::Win32::Foundation::{E_INVALIDARG, RECT};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{BOOL, Interface};

pub(super) unsafe extern "system" fn root(list: &List, signature: *mut c_void) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    match unsafe { root_signature(signature, parent(&list.parent)) } {
        Ok(root) => unsafe { list.native.SetGraphicsRootSignature(root) },
        Err(error) => state.error = error,
    }
}

pub(super) unsafe extern "system" fn topology(list: &List, primitive: D3D_PRIMITIVE_TOPOLOGY) {
    let mut state = recording(list);
    if state.can_record(&list.native) {
        unsafe { list.native.IASetPrimitiveTopology(primitive) };
    }
}

pub(super) unsafe extern "system" fn pipeline(list: &List, guest: *mut c_void) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    match unsafe { crate::device::compute::pipeline(guest, parent(&list.parent)) } {
        Ok(Some(host)) => unsafe { list.native.SetPipelineState(host) },
        _ => state.error = E_INVALIDARG,
    }
}

pub(super) unsafe extern "system" fn viewports(
    list: &List,
    count: u32,
    views: *const D3D12_VIEWPORT,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if count > D3D12_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE
        || (count != 0 && views.is_null())
    {
        state.error = E_INVALIDARG;
        return;
    }
    unsafe { (list.native.vtable().RSSetViewports)(list.native.as_raw(), count, views) };
}

pub(super) unsafe extern "system" fn scissors(list: &List, count: u32, rects: *const RECT) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if count > D3D12_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE
        || (count != 0 && rects.is_null())
    {
        state.error = E_INVALIDARG;
        return;
    }
    unsafe { (list.native.vtable().RSSetScissorRects)(list.native.as_raw(), count, rects) };
}

// At RVA 0x5790ccf the title passes the count and handle array in RDX/R8,
// the range flag in R9, and a pointer to an optional DSV on the stack.
pub(super) unsafe extern "system" fn targets(
    list: &List,
    count: u32,
    rtvs: *const D3D12_CPU_DESCRIPTOR_HANDLE,
    contiguous: BOOL,
    dsv: *const D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if count > D3D12_SIMULTANEOUS_RENDER_TARGET_COUNT || (count != 0 && rtvs.is_null()) {
        state.error = E_INVALIDARG;
        return;
    }
    unsafe {
        (list.native.vtable().OMSetRenderTargets)(
            list.native.as_raw(),
            count,
            rtvs,
            contiguous,
            dsv,
        )
    };
}

pub(super) unsafe extern "system" fn draw(
    list: &List,
    vertices: u32,
    instances: u32,
    start_vertex: u32,
    start_instance: u32,
) {
    let mut state = recording(list);
    if state.can_record(&list.native) {
        state.sample_heap();
        unsafe {
            list.native
                .DrawInstanced(vertices, instances, start_vertex, start_instance)
        };
    }
}

// FD3D12CommandContext::RHIDispatchMeshShader calls slot 127 at Frosty
// RVA 0x578f241 with X/Y/Z group counts in the standard register order.
pub(super) unsafe extern "system" fn dispatch_mesh(list: &List, x: u32, y: u32, z: u32) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if x.max(y).max(z) > D3D12_CS_DISPATCH_MAX_THREAD_GROUPS_PER_DIMENSION
        || u64::from(x) * u64::from(y) * u64::from(z) > 1 << 22
        || !matches!(
            unsafe { list.native.GetType() },
            D3D12_COMMAND_LIST_TYPE_DIRECT | D3D12_COMMAND_LIST_TYPE_BUNDLE
        )
    {
        state.error = E_INVALIDARG;
        return;
    }
    let Some(native) = list.state.mesh.get_or_init(|| list.native.cast().ok()) else {
        state.error = windows::Win32::Foundation::E_NOINTERFACE;
        return;
    };
    state.sample_heap();
    unsafe { native.DispatchMesh(x, y, z) };
    static FIRST_MESH: std::sync::Once = std::sync::Once::new();
    crate::diagnostics::log_once(&FIRST_MESH, || {
        format!("recorded Xbox slot 127 DispatchMesh ({x}, {y}, {z})")
    });
}

// Frosty's RHIDrawIndexedPrimitive chooses DXGI_FORMAT_R16_UINT or R32_UINT
// at RVA 0x578ec32 and sends the packed GPU address, size and format to slot 47.
pub(super) unsafe extern "system" fn index_buffer(
    list: &List,
    view: *const D3D12_INDEX_BUFFER_VIEW,
) {
    let mut state = recording(list);
    if state.can_record(&list.native) {
        unsafe { (list.native.vtable().IASetIndexBuffer)(list.native.as_raw(), view) };
    }
}

pub(super) unsafe extern "system" fn vertex_buffers(
    list: &List,
    start: u32,
    count: u32,
    views: *const D3D12_VERTEX_BUFFER_VIEW,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if start > D3D12_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT
        || count > D3D12_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT - start
    {
        state.error = E_INVALIDARG;
        return;
    }
    // The views already contain GPU addresses from the native buffers. A null
    // view array clears the requested vertex slots.
    unsafe { (list.native.vtable().IASetVertexBuffers)(list.native.as_raw(), start, count, views) };
}

// Slot 13 at Frosty RVA 0x578ecf6 uses the desktop five-argument ordering.
pub(super) unsafe extern "system" fn draw_indexed(
    list: &List,
    indices: u32,
    instances: u32,
    first: u32,
    base_vertex: i32,
    first_instance: u32,
) {
    let mut state = recording(list);
    if state.can_record(&list.native) {
        state.sample_heap();
        unsafe {
            list.native
                .DrawIndexedInstanced(indices, instances, first, base_vertex, first_instance)
        };
    }
}

// Frosty's ApplyState records these output-merger values at RVAs 0x57f0356 and 0x57f0398.
pub(super) unsafe extern "system" fn blend_factor(list: &List, factor: *const f32) {
    let mut state = recording(list);
    if state.can_record(&list.native) {
        unsafe { (list.native.vtable().OMSetBlendFactor)(list.native.as_raw(), factor) };
    }
}

pub(super) unsafe extern "system" fn stencil_ref(list: &List, reference: u32) {
    let mut state = recording(list);
    if state.can_record(&list.native) {
        unsafe { list.native.OMSetStencilRef(reference) };
    }
}

// ApplyState loads the depth minimum and maximum into XMM1/XMM2 before the
// slot 68 call returning at title RVA 0x57f03ed.
pub(super) unsafe extern "system" fn depth_bounds(list: &List, minimum: f32, maximum: f32) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    let native = list
        .state
        .extended
        .get_or_init(|| list.native.cast::<ID3D12GraphicsCommandList1>().ok());
    match native {
        Some(native) => unsafe { native.OMSetDepthBounds(minimum, maximum) },
        None => state.error = windows::Win32::Foundation::E_NOINTERFACE,
    }
}
