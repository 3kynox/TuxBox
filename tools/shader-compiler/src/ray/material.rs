//! Native material hit programs run once for the committed triangle. The shader record
//! supplies geometry addresses, material bindings and instance constants.
use super::{continuation, fingerprint};
use tuxbox_dxbc::ray::State;

// These programs share the local geometry ABI.
// The export string can vary independently of the shader machine code.
pub(super) fn matches(state: &State<'_>) -> bool {
    if state.kind != 2 || state.hit_group.is_none() || state.any_hit.is_some() {
        return false;
    }
    matches!(
        (state.program.len(), fingerprint(state.program, &[])),
        (
            12904,
            0x4c94e2fd52a4b6f8 | 0x11dd92e0a5b8c4bf | 0x96ee9bb2322dd98f
        ) | (14440, 0xd7165b7f96d588f5)
            | (14696, 0xc202a91f516314be)
            | (14952, 0x7dd0b9d85c844854)
            | (
                16232,
                0xc84111c265b9ff2b | 0xd8a742a8e89a619e | 0xa93af1aed4d66d08
            )
            | (16744, 0x6ddcd0a3da590492)
            | (21864, 0x568a912ba12c4ef7)
            | (22120, 0xb62fdb4066c307de)
            | (2920, 0xe2c0c21503dcc5d4)
    )
}

pub(super) fn translated(state: &State<'_>) -> Result<String, String> {
    // The Lumen geometry hit writes distance, geometry user data, normal and
    // two instance indices. The full material programs have eleven more words.
    let words = if state.program.len() == 2920 { 5 } else { 16 };
    let leaf = continuation::leaf_body(state, words)?;
    if state.any_hit.is_some() {
        return Err("unsupported material any-hit export".into());
    }
    // v2 and v3 identify the instance and triangle nodes. The memory lowerer
    // reconstructs their fields from DXR intrinsics. The encoded scene base
    // in v16 and the local record base in v5 are private to that lowering.
    Ok(format!(
        r#"{declarations}
struct MaterialPayload {{ uint words[{words}]; }};
[shader("closesthit")]
void {name}(inout MaterialPayload payload, BuiltInTriangleIntersectionAttributes attributes) {{
    uint s[128] = (uint[128])0, v[128] = (uint[128])0;
    uint exec = ~0u, scc = 1u, vcc = 0u, m0 = 0u;
    uint spill[{frame}] = (uint[{frame}])0;
    [unroll] for (uint i = 0; i < {words}; i++) spill[i] = payload.words[i];
    v[2] = 8u; v[3] = 8u; v[4] = HitKind(); v[6] = asuint(RayTCurrent());
    v[7] = {words}u; v[14] = asuint(attributes.barycentrics.x); v[15] = asuint(attributes.barycentrics.y);
    v[16] = 0x00300000u;
    uint3 origin = asuint(WorldRayOrigin()), direction = asuint(WorldRayDirection());
    v[19] = origin.x; v[20] = origin.y; v[21] = origin.z; v[22] = asuint(RayTMin());
    v[23] = direction.x; v[24] = direction.y; v[25] = direction.z;
    {body}
    [unroll] for (uint i = 0; i < {words}; i++) payload.words[i] = spill[i];
}}
"#,
        frame = words + 1,
        name = state.name,
        declarations = leaf.declarations,
        body = leaf.body
    ))
}
