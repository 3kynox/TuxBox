// Standalone whole-body verification control; intentionally one GPU thread.
// All buffers use bounded descriptors. Caller must synchronize source writes.
ByteAddressBuffer source : register(t0);
ByteAddressBuffer validated : register(t1);
RWByteAddressBuffer shadow : register(u0);
RWByteAddressBuffer status : register(u1);
cbuffer Count : register(b0) { uint bytes; }
[numthreads(1, 1, 1)] void main() {
    uint sourceBytes, validatedBytes, shadowBytes, statusBytes;
    source.GetDimensions(sourceBytes);
    validated.GetDimensions(validatedBytes);
    shadow.GetDimensions(shadowBytes);
    status.GetDimensions(statusBytes);
    if (statusBytes < 4) return;
    if (bytes == 0 || (bytes & 3) != 0 || bytes > 65536 ||
        bytes > sourceBytes || bytes > validatedBytes || bytes > shadowBytes) {
        status.Store(0, 3);
        return;
    }
    uint difference = 0;
    [loop] for (uint offset = 0; offset < bytes; offset += 4)
        difference |= source.Load(offset) ^ validated.Load(offset);
    if (difference != 0) {
        status.Store(0, 2);
        return;
    }
    [loop] for (uint offset = 0; offset < bytes; offset += 4)
        shadow.Store(offset, source.Load(offset));
    status.Store(0, 1);
}
