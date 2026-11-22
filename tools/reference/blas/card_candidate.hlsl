// Limited two-segment hypothesis for the controlled direct-light fixture.
// This is not a translation of packaged compute shader 219.
RaytracingAccelerationStructure scene : register(t0);
StructuredBuffer<float3> bounds : register(t1);
RWStructuredBuffer<uint4> result : register(u0);

bool trace(float minimum, float maximum, float origin_x) {
    RayDesc ray = {float3(origin_x, 0, -1), minimum, float3(0, 0, 1), maximum};
    RayQuery<RAY_FLAG_FORCE_OPAQUE | RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH> query;
    query.TraceRayInline(scene, RAY_FLAG_NONE, 0xff, ray);
    while (query.Proceed()) {}
    return query.CommittedStatus() == COMMITTED_TRIANGLE_HIT;
}

[numthreads(1, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    float3 interval = bounds[id.x];
    bool first = trace(interval.x, interval.y, interval.z);
    bool second = false;
    if (!first) second = trace(0, interval.x, interval.z);
    result[id.x] = uint4(first, second, 2 | uint(first || second), 0);
}
