// Experimental bounded TRI0 -> procedural AABB conversion. A failed status
// invalidates the entire output; this shader does not itself block an AS build.
ByteAddressBuffer leaves : register(t0);
RWByteAddressBuffer bounds : register(u0);
RWByteAddressBuffer status : register(u1);
cbuffer Sizes : register(b0) { uint leaf_bytes; uint bounds_bytes; uint count; }

float outward(float value, bool upper) {
    uint bits = asuint(value);
    // Expand tiny values across zero by a normal amount. Subnormal bounds
    // could otherwise be flushed inward by a driver or traversal unit.
    if ((bits & 0x7fffffffu) < 0x35800000u)
        return asfloat(upper ? 0x35800000u : 0xb5800000u);
    bool positive = (bits & 0x80000000u) == 0;
    return asfloat(bits + ((positive == upper) ? 1u : 0xffffffffu));
}

[numthreads(64, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    if (id.x >= count) return;
    if (count > leaf_bytes / 64 || count > bounds_bytes / 24) {
        uint ignored; status.InterlockedOr(0, 1, ignored); return;
    }
    uint offset = id.x * 64;
    uint descriptor = leaves.Load(offset + 48);
    uint second = leaves.Load(offset + 56);
    uint selectors = leaves.Load(offset + 60);
    float3 a = asfloat(leaves.Load3(offset));
    float3 b = asfloat(leaves.Load3(offset + 12));
    float3 c = asfloat(leaves.Load3(offset + 24));
    if ((descriptor & 255u) != 0 || second != 0xffffffffu ||
        (selectors != 2 && selectors != 4 && selectors != 9) ||
        !all(isfinite(a)) || !all(isfinite(b)) || !all(isfinite(c))) {
        uint ignored; status.InterlockedOr(0, 2, ignored); return;
    }
    float3 lo = min(a, min(b, c)), hi = max(a, max(b, c));
    lo = float3(outward(lo.x, false), outward(lo.y, false), outward(lo.z, false));
    hi = float3(outward(hi.x, true), outward(hi.y, true), outward(hi.z, true));
    if (!all(isfinite(lo)) || !all(isfinite(hi))) {
        uint ignored; status.InterlockedOr(0, 2, ignored); return;
    }
    bounds.Store3(id.x * 24, asuint(lo));
    bounds.Store3(id.x * 24 + 12, asuint(hi));
}
