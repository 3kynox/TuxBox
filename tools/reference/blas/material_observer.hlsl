struct NativeMaterialPayload {uint words[16];};
RaytracingAccelerationStructure scene : register(t0,space3);
RWStructuredBuffer<uint> result : register(u0,space3);

[shader("raygeneration")]
void MaterialProbeRay() {
    uint i = DispatchRaysIndex().x;
    bool reverse = (i & 1u) != 0;
    float3 pos = float3(i >= 4 ? 0 : i < 2 ? -1 : 3, 0, reverse ? 1 : -1);
    RayDesc ray = {pos, 0, float3(0,0,reverse ? -1 : 1), 2};
    NativeMaterialPayload p = (NativeMaterialPayload)0;
    [unroll] for (uint j = 0; j < 16; ++j) p.words[j] = 0x00001000u + j;
    p.words[9] = i < 2 ? 0u : 1u << 25;
    TraceRay(scene, RAY_FLAG_FORCE_OPAQUE, 255, 0, 0, 0, ray, p);
    [unroll] for (uint j = 0; j < 16; ++j) result[16*i+j] = p.words[j];
}

[shader("miss")]
void MaterialProbeMiss(inout NativeMaterialPayload p) {
    p.words[0] = asuint(-1.0);
}
