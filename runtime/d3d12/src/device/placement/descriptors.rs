//! Sparse shadow of placed SRVs in CPU and shader-visible descriptor heaps.
//! All other descriptors continue to pass directly to the native device.
use super::{Texture, lock};
use std::{
    collections::BTreeMap,
    ops::Range,
    sync::{Arc, Mutex, Weak},
};
use windows::Win32::Graphics::Direct3D12::*;

#[derive(Default)]
pub(crate) struct Views(Mutex<BTreeMap<usize, Weak<Texture>>>);

pub(crate) struct HeapViews {
    views: Arc<Views>,
    pub(crate) range: Range<usize>,
}

impl Drop for HeapViews {
    fn drop(&mut self) {
        lock(&self.views.0)
            .extract_if(self.range.clone(), |_, _| true)
            .for_each(drop);
    }
}

impl Views {
    pub(crate) fn heap(
        self: &Arc<Self>,
        device: &ID3D12Device,
        native: &ID3D12DescriptorHeap,
        desc: &D3D12_DESCRIPTOR_HEAP_DESC,
    ) -> Option<HeapViews> {
        if desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV {
            return None;
        }
        let start = unsafe { native.GetCPUDescriptorHandleForHeapStart() }.ptr;
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) } as usize;
        Some(HeapViews {
            views: self.clone(),
            range: start..start + stride * desc.NumDescriptors as usize,
        })
    }

    pub(crate) fn set(&self, handle: usize, texture: Option<&Arc<Texture>>) {
        let mut views = lock(&self.0);
        if let Some(texture) = texture {
            views.insert(handle, Arc::downgrade(texture));
        } else {
            views.remove(&handle);
        }
    }

    pub(crate) fn gather(&self, heaps: &[Range<usize>], textures: &mut Vec<Arc<Texture>>) {
        if !heaps.is_empty() {
            let views = lock(&self.0);
            for heap in heaps {
                textures.extend(
                    views
                        .range(heap.clone())
                        .filter_map(|(_, view)| view.upgrade()),
                );
            }
        }
    }

    pub(crate) fn copy(
        &self,
        stride: usize,
        destinations: impl Iterator<Item = (usize, u32)>,
        sources: impl Iterator<Item = (usize, u32)>,
    ) {
        let mut views = lock(&self.0);
        if views.is_empty() {
            return;
        }
        // The source and destination descriptor counts match, but their range
        // boundaries need not. Null size arrays default to one slot per range.
        let mut dst = destinations.filter(|(_, count)| *count > 0);
        let mut src = sources.filter(|(_, count)| *count > 0);
        let mut d = dst.next();
        let mut s = src.next();
        let mut copied = Vec::new();
        while let (Some((to, left)), Some((from, available))) = (d, s) {
            let count = left.min(available);
            let span = count as usize * stride;
            copied.extend(
                views
                    .range(from..from + span)
                    .map(|(&key, value)| (to + (key - from), value.clone())),
            );
            views.extract_if(to..to + span, |_, _| true).for_each(drop);
            views.extend(copied.drain(..));
            d = if count == left {
                dst.next()
            } else {
                Some((to + span, left - count))
            };
            s = if count == available {
                src.next()
            } else {
                Some((from + span, available - count))
            };
        }
    }
}
