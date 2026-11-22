// Tests whether native procedural AABB candidates can expose t=0. This is not
// an Xbox triangle intersection implementation or a shader 219 translation.
RaytracingAccelerationStructure scene : register(t0);
StructuredBuffer<float4> origins : register(t1);
RWStructuredBuffer<uint4> results : register(u0);

[numthreads(1, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    RayDesc ray = {origins[id.x].xyz, 0, float3(0, 0, 1), 2};
    RayQuery<RAY_FLAG_NONE> query;
    query.TraceRayInline(scene, RAY_FLAG_NONE, 0xff, ray);
    uint candidates = 0;
    while (query.Proceed()) {
        if (query.CandidateType() == CANDIDATE_PROCEDURAL_PRIMITIVE) {
            candidates++;
            // Only the strictly interior, on-plane point of this fixed test
            // triangle is accepted. General Xbox edge/math is not modelled.
            float3 o = ray.Origin;
            if (o.z == 0 && o.x > 0 && o.y > 0 && o.x + o.y < 1)
                query.CommitProceduralPrimitiveHit(0);
        }
    }
    uint status = query.CommittedStatus();
    results[id.x] = uint4(candidates, status, status ? asuint(query.CommittedRayT()) : 0, 0);
}
