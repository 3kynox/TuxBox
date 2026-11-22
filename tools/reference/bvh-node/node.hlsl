// Scalar reference operations for Xbox runtime-style BVH4 records.
// Scope: finite inputs, nonzero ray direction components, zero box growth,
// closest-first sorting, and uncompressed single triangles. This is not a
// hardware-exact replacement for image_bvh64_intersect_ray.
float tb_bvh_bound(uint node[32], uint child, uint component, bool box16) {
    if (!box16)
        return asfloat(node[4 + child * 6 + component]);
    uint halfIndex = child * 6 + component;
    uint pair = node[4 + halfIndex / 2];
    return f16tof32((pair >> ((halfIndex & 1u) * 16u)) & 0xffffu);
}

uint4 tb_bvh_boxes(uint node[32], bool box16, float3 origin,
                   float3 inverseDirection, float extent, bool sortChildren) {
    uint child[4];
    float distance[4];
    [unroll] for (uint i = 0; i < 4; ++i) {
        float3 lower = float3(tb_bvh_bound(node, i, 0, box16),
                              tb_bvh_bound(node, i, 1, box16),
                              tb_bvh_bound(node, i, 2, box16));
        float3 upper = float3(tb_bvh_bound(node, i, 3, box16),
                              tb_bvh_bound(node, i, 4, box16),
                              tb_bvh_bound(node, i, 5, box16));
        float3 a = (lower - origin) * inverseDirection;
        float3 b = (upper - origin) * inverseDirection;
        float3 nearAxis = min(a, b);
        float3 farAxis = max(a, b);
        float nearT = max(0.0, max(nearAxis.x, max(nearAxis.y, nearAxis.z)));
        float farT = min(extent, min(farAxis.x, min(farAxis.y, farAxis.z)));
        child[i] = nearT <= farT ? node[i] : 0xffffffffu;
        distance[i] = nearT;
    }
    if (sortChildren) {
        // Stable insertion sort keeps the node order for equal entry distances.
        [unroll] for (uint i = 1; i < 4; ++i) {
            [unroll] for (uint j = i; j > 0; --j) {
                bool swap = child[j - 1] == 0xffffffffu ||
                    (child[j] != 0xffffffffu && distance[j] < distance[j - 1]);
                if (swap) {
                    uint c = child[j - 1]; child[j - 1] = child[j]; child[j] = c;
                    float d = distance[j - 1]; distance[j - 1] = distance[j]; distance[j] = d;
                }
            }
        }
    }
    return uint4(child[0], child[1], child[2], child[3]);
}

// Returns unremapped t numerator, determinant, and two barycentric
// numerators. A miss returns +infinity and one in the first two words.
float4 tb_bvh_triangle(float3 origin, float3 direction,
                       float3 a, float3 b, float3 c) {
    float3 edge1 = b - a;
    float3 edge2 = c - a;
    float3 fromVertex = origin - a;
    float3 p = cross(direction, edge2);
    float3 q = cross(fromVertex, edge1);
    float determinant = dot(edge1, p);
    float tNumerator = dot(edge2, q);
    float iNumerator = dot(fromVertex, p);
    float jNumerator = dot(direction, q);
    if (determinant == 0.0)
        return float4(asfloat(0x7f800000u), 1.0, iNumerator, jNumerator);
    float t = tNumerator / determinant;
    float i = iNumerator / determinant;
    float j = jNumerator / determinant;
    if (t < 0.0 || i < 0.0 || j < 0.0 || i + j > 1.0)
        return float4(asfloat(0x7f800000u), 1.0, iNumerator, jNumerator);
    return float4(tNumerator, determinant, iNumerator, jNumerator);
}

// First uncompressed triangle in a 64-byte Xbox leaf record. The vertex
// positions occupy its first nine dwords. Barycentric remapping and the
// other triangle node types are deliberately left to their callers.
float4 tb_bvh_triangle_leaf(uint node[32], float3 origin, float3 direction) {
    float3 a = asfloat(uint3(node[0], node[1], node[2]));
    float3 b = asfloat(uint3(node[3], node[4], node[5]));
    float3 c = asfloat(uint3(node[6], node[7], node[8]));
    return tb_bvh_triangle(origin, direction, a, b, c);
}
