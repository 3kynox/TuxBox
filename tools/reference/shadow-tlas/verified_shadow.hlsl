// Standalone whole-body verification control; intentionally one GPU thread.
// Caller must validate all view extents and synchronize source writes.
ByteAddressBuffer source : register(t0);
ByteAddressBuffer validated : register(t1);
RWByteAddressBuffer shadow : register(u0);
RWByteAddressBuffer status : register(u1);
cbuffer Count : register(b0) { uint bytes; }
[numthreads(1, 1, 1)] void main() {
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
