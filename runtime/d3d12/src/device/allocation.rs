//! Frosty reads a 16-byte result and hashes each 56-byte resource description.
//! Slot 25 passes the result in RCX and the receiver in RDX (title RVA 0x57945e9).
use super::Device;
use core::slice;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

const _: () = assert!(
    size_of::<D3D12_RESOURCE_DESC>() == 56 && size_of::<D3D12_RESOURCE_ALLOCATION_INFO>() == 16
);

pub(super) unsafe extern "system" fn info(
    out: *mut D3D12_RESOURCE_ALLOCATION_INFO,
    device: &Device,
    visible: u32,
    count: u32,
    descs: *const D3D12_RESOURCE_DESC,
) -> *mut D3D12_RESOURCE_ALLOCATION_INFO {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return out;
    };
    let invalid = D3D12_RESOURCE_ALLOCATION_INFO {
        SizeInBytes: u64::MAX,
        Alignment: 0,
    };
    if descs.is_null() || count == 0 || visible > 1 {
        *out = invalid;
        return out;
    }
    let descs = unsafe { slice::from_raw_parts(descs, count as usize) };
    let supported = descs.iter().all(|d| {
        matches!(
            d.Dimension,
            D3D12_RESOURCE_DIMENSION_BUFFER
                | D3D12_RESOURCE_DIMENSION_TEXTURE1D
                | D3D12_RESOURCE_DIMENSION_TEXTURE2D
                | D3D12_RESOURCE_DIMENSION_TEXTURE3D
        ) && matches!(
            d.Layout,
            D3D12_TEXTURE_LAYOUT_UNKNOWN | D3D12_TEXTURE_LAYOUT_ROW_MAJOR
        ) && d.Width != 0
            && d.Height != 0
            && d.DepthOrArraySize != 0
            && d.SampleDesc.Count != 0
    });
    *out = if supported {
        // Let the same adapter that creates the placed resource determine its
        // requirements, including padding between descriptors in a batch.
        if descs.iter().any(|d| super::depth::primary(d).is_some()) {
            let mut host = Vec::new();
            if host.try_reserve_exact(descs.len()).is_err() {
                *out = invalid;
                return out;
            }
            host.extend(descs.iter().map(|d| super::depth::primary(d).unwrap_or(*d)));
            unsafe { device.native.GetResourceAllocationInfo(visible, &host) }
        } else {
            unsafe { device.native.GetResourceAllocationInfo(visible, descs) }
        }
    } else {
        invalid
    };
    out
}

// The shipping texture upload routine (RVA 0x57e12a1) supplies independent
// arrays with strides 32, 4 and 8 for layout, row count and unpadded row size.
pub(super) unsafe extern "system" fn footprints(
    device: &Device,
    desc: *const D3D12_RESOURCE_DESC,
    first: u32,
    count: u32,
    base: u64,
    layouts: *mut D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    rows: *mut u32,
    row_bytes: *mut u64,
    total: *mut u64,
) {
    const _: () = assert!(size_of::<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>() == 32);
    let primary = unsafe { desc.as_ref() }.and_then(super::depth::primary);
    let desc = primary.as_ref().map_or(desc, core::ptr::from_ref);
    // All four output arrays are optional. Their storage is owned by the guest.
    unsafe {
        (device.native.vtable().GetCopyableFootprints)(
            device.native.as_raw(),
            desc,
            first,
            count,
            base,
            layouts,
            rows,
            row_bytes,
            total,
        );
    }
}
