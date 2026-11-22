// Bounded GPU directory control. This is not a runtime lookup implementation.
ByteAddressBuffer directory : register(t0);
ByteAddressBuffer queries : register(t1);
RWByteAddressBuffer result : register(u0);
cbuffer Counts : register(b0) { uint entryCount; uint queryCount; }

// Status 0 = absent, 1 = unique match, 2 = invalid or ambiguous,
// 3 = count outside this control's allocated capacities.
[numthreads(8,1,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (id.x >= min(queryCount, 8u)) return;
    if (entryCount > 5 || queryCount > 8) {
        result.Store4(id.x * 16, uint4(0, 0, 3, 0));
        return;
    }
    uint2 key = queries.Load2(id.x * 8);
    if ((key.x | key.y) == 0 || (key.x & 255) != 0 || key.y >= 256) {
        result.Store4(id.x * 16, uint4(0, 0, 2, 0));
        return;
    }
    uint2 target = 0;
    uint matches = 0;
    for (uint i = 0; i < entryCount; ++i) {
        uint4 record = directory.Load4(i * 16);
        if (all(record.xy == key)) {
            ++matches;
            target = record.zw;
        }
    }
    uint status = matches == 0 ? 0 : matches == 1 && (target.x | target.y) != 0 &&
                  (target.x & 255) == 0 ? 1 : 2;
    result.Store4(id.x * 16, status == 1 ? uint4(target, status, 0) : uint4(0, 0, status, 0));
}
