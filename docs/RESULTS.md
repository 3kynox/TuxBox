# Measured homebrew results

Each column represents one run on its named host, provider and hardware. Each receipt records the guest output, exit status and binary hashes.

| Gate | [ge-nvidia](../evidence/ge-nvidia.json) | [valve-amd](../evidence/valve-amd.json) | [valve-nvidia](../evidence/valve-nvidia.json) | [wine-nvidia](../evidence/wine-nvidia.json) | Physical Windows x64 |
|---|---|---|---|---|---|
| Upper route and guest output | Pass | Pass | Pass | Pass | Pending |
| Lower route and collision | Pass | Pass | Pass | Pass | Pending |
| Invalid input exit status | Pass | Pass | Pass | Pass | Pending |
| Xbox DXC utilities and binary blob lifetime | Pass | Pass | Pass | Pass | Pending |
| Native Scarlett XG texture layouts with Rust XMem | Pass | Pass | Pass | Pass | Pending |
| Xbox memory allocation, working set and CPU pages | Pass | Pass | Pass | Pass | Pending |
| Desktop D3D12 pixel readback | Pass | Pass | Pass | Pass | Pending |
| Xbox root-signature import and compute readback | Pass | Pass | Pass | Pass | Pending |
| Xbox command-list clear and ordered copy readback | Pass | Pass | Pass | Fail | Pending |
| Guest command rejection, fence isolation and list recovery | Pass | Pass | Pass | Fail | Pending |
| Xbox PresentX and two screen-verified frames | Fail | Fail | Pass | Fail | Pending |
| Lanes package launch and gameplay replay | Pass | Pass | Pass | Pass | Pending |
| 8,388,608-move CPU replay | Pass | Pass | Pass | Pass | Pending |

Rebuild this table from the repository root with `python tuxbox.py status > docs/RESULTS.md`. A dash means that the gate was omitted from that run. [Xbox DLL checks](LIBRARIES.md#run-the-provider-checks) · [Milestones and retail evidence](PROGRESS.md)
