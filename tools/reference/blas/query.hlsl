RaytracingAccelerationStructure scene : register(t0);
RWStructuredBuffer<uint> results : register(u0);

[numthreads(5, 1, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    RayDesc ray;
    ray.Origin = float3(float(tid.x) * 2.0 - 1.0, 0, -1);
    ray.Direction = float3(0, 0, 1);
    ray.TMin = 0;
    ray.TMax = 10;
    RayQuery<RAY_FLAG_NONE> query;
    query.TraceRayInline(scene, RAY_FLAG_NONE, 0xff, ray);
    while (query.Proceed()) {}
    results[tid.x] = query.CommittedStatus() == COMMITTED_TRIANGLE_HIT
        ? (query.CommittedInstanceID() << 16) | (query.CommittedGeometryIndex() << 8)
          | uint(round(16.0 * query.CommittedRayT()))
        : 0xdead;
}
