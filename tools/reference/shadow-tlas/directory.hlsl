// Isolated one-entry publication control; no runtime lookup implementation.
ByteAddressBuffer directory : register(t0);
RWByteAddressBuffer result : register(u0);
[numthreads(1,1,1)] void main() {
    result.Store4(0, directory.Load4(0));
}
