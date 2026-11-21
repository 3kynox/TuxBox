#include "frame_abi.h"
#include <fstream>
#include <iterator>
#include <vector>
using frame_abi::check;
using frame_abi::ComPtr;

template <typename T> struct alignas(void *) Part {
  UINT tag;
  T payload;
};
struct Stream {
  Part<UINT> flags{20, 0};
  Part<ID3D12RootSignature *> root{0, nullptr};
  Part<D3D12_INPUT_LAYOUT_DESC> layout{12, {}};
  Part<UINT> strip{13, 0}, topology{14, D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE};
  Part<D3D12_SHADER_BYTECODE> vs{1, {}}, gs{5, {}}, ps{2, {}};
  Part<D3D12_BLEND_DESC> blend{8, {}};
  Part<D3D12_DEPTH_STENCIL_DESC1> depth{21, {}};
  Part<DXGI_FORMAT> dsv{16, DXGI_FORMAT_D32_FLOAT};
  Part<D3D12_RASTERIZER_DESC> raster{10, {}};
  Part<D3D12_RT_FORMAT_ARRAY> formats{15, {}};
  Part<DXGI_SAMPLE_DESC> samples{17, {1, 0}};
  Part<UINT> mask{9, ~0u};
  Part<D3D12_CACHED_PIPELINE_STATE> cached{19, {}};
  UINT extension = 26;
  BYTE stencil[28]{};
};
static_assert(offsetof(Stream, vs) == 0x40 &&
              offsetof(Stream, extension) == 0x2a8 && sizeof(Stream) == 0x2c8);
static std::vector<char> read(const char *name) {
  std::ifstream file(name, std::ios::binary);
  if (!file)
    throw std::runtime_error(name);
  return {std::istreambuf_iterator<char>{file},
          std::istreambuf_iterator<char>{}};
}
static ComPtr<ID3D12Resource> resource(ID3D12Device *dev, bool depth) {
  D3D12_HEAP_PROPERTIES hp{};
  hp.Type = depth ? D3D12_HEAP_TYPE_DEFAULT : D3D12_HEAP_TYPE_READBACK;
  hp.CreationNodeMask = hp.VisibleNodeMask = 1;
  D3D12_RESOURCE_DESC desc{};
  desc.Dimension = depth ? D3D12_RESOURCE_DIMENSION_TEXTURE2D
                         : D3D12_RESOURCE_DIMENSION_BUFFER;
  desc.Width = depth ? 8 : 8 * 256;
  desc.Height = depth ? 8 : 1;
  desc.DepthOrArraySize = desc.MipLevels = desc.SampleDesc.Count = 1;
  desc.Format = depth ? DXGI_FORMAT_D32_FLOAT : DXGI_FORMAT_UNKNOWN;
  desc.Layout =
      depth ? D3D12_TEXTURE_LAYOUT_UNKNOWN : D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
  desc.Flags = depth ? D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL
                     : D3D12_RESOURCE_FLAG_NONE;
  ComPtr<ID3D12Resource> result;
  check(dev->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &desc,
                                     depth ? D3D12_RESOURCE_STATE_DEPTH_WRITE
                                           : D3D12_RESOURCE_STATE_COPY_DEST,
                                     nullptr, IID_PPV_ARGS(&result)),
        "create resource");
  return result;
}
static UINT draw(ID3D12Device *dev, ID3D12PipelineState *pso,
                 ID3D12RootSignature *root) {
  auto depth = resource(dev, true), readback = resource(dev, false);
  ComPtr<ID3D12DescriptorHeap> dsv;
  D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_DSV, 1,
                                  D3D12_DESCRIPTOR_HEAP_FLAG_NONE, 0};
  check(dev->CreateDescriptorHeap(&heap, IID_PPV_ARGS(&dsv)), "DSV heap");
  auto handle = dsv->GetCPUDescriptorHandleForHeapStart();
  dev->CreateDepthStencilView(depth.Get(), nullptr, handle);
  ComPtr<ID3D12CommandAllocator> alloc;
  ComPtr<ID3D12CommandQueue> queue;
  ComPtr<ID3D12GraphicsCommandList> list;
  D3D12_COMMAND_QUEUE_DESC q{};
  check(dev->CreateCommandQueue(&q, IID_PPV_ARGS(&queue)), "queue");
  check(dev->CreateCommandAllocator(q.Type, IID_PPV_ARGS(&alloc)), "allocator");
  check(
      dev->CreateCommandList(0, q.Type, alloc.Get(), pso, IID_PPV_ARGS(&list)),
      "list");
  list->OMSetRenderTargets(0, nullptr, FALSE, &handle);
  list->ClearDepthStencilView(handle, D3D12_CLEAR_FLAG_DEPTH, 1, 0, 0, nullptr);
  list->SetGraphicsRootSignature(root);
  D3D12_VIEWPORT vp{0, 0, 8, 8, 0, 1};
  D3D12_RECT rect{0, 0, 8, 8};
  list->RSSetViewports(1, &vp);
  list->RSSetScissorRects(1, &rect);
  list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
  list->DrawInstanced(3, 1, 0, 0);
  D3D12_RESOURCE_BARRIER barrier{};
  barrier.Transition = {depth.Get(), 0, D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        D3D12_RESOURCE_STATE_COPY_SOURCE};
  list->ResourceBarrier(1, &barrier);
  D3D12_TEXTURE_COPY_LOCATION dst{}, src{};
  dst.pResource = readback.Get();
  dst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
  dst.PlacedFootprint.Footprint = {DXGI_FORMAT_D32_FLOAT, 8, 8, 1, 256};
  src.pResource = depth.Get();
  src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
  list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
  check(list->Close(), "close");
  ID3D12CommandList *lists[] = {list.Get()};
  queue->ExecuteCommandLists(1, lists);
  ComPtr<ID3D12Fence> fence;
  check(dev->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)),
        "fence");
  check(queue->Signal(fence.Get(), 1), "signal");
  HANDLE e = CreateEventW(nullptr, FALSE, FALSE, nullptr);
  if (!e)
    throw std::runtime_error("event");
  check(fence->SetEventOnCompletion(1, e), "set completion");
  auto waited = WaitForSingleObject(e, 15000);
  CloseHandle(e);
  if (waited != WAIT_OBJECT_0)
    throw std::runtime_error("GPU timeout");
  D3D12_RANGE range{0, 2048}, none{};
  void *data = nullptr;
  check(readback->Map(0, &range, &data), "map");
  UINT changed = 0;
  for (UINT y = 0; y < 8; ++y)
    for (UINT x = 0; x < 8; ++x) {
      float value = static_cast<float *>(data)[64 * y + x];
      if (value == .25f)
        ++changed;
      else if (value != 1.f)
        throw std::runtime_error("unexpected depth");
    }
  readback->Unmap(0, &none);
  return changed;
}
static void run(FILE *file) {
  auto dll = LoadLibraryW(L"d3d12_xs.dll");
  using Create = HRESULT(WINAPI *)(IUnknown *, const void *, REFIID, void **);
  auto create = dll ? reinterpret_cast<Create>(
                          GetProcAddress(dll, "D3D12XboxCreateDevice"))
                    : nullptr;
  if (!create)
    throw std::runtime_error("device factory");
  std::array<UINT, 18> profile{0x20004, 0, ~0u, 0, 0, 0, ~0u, ~0u, 0, 1};
  ComPtr<ID3D12Device> provider;
  check(create(nullptr, profile.data(), IID_PPV_ARGS(&provider)),
        "Xbox device");
  auto original = read("mat6113.dxbc");
  ComPtr<ID3D12RootSignature> root;
  check(provider->CreateRootSignature(0, original.data(), original.size(),
                                      IID_PPV_ARGS(&root)),
        "title root");
  ComPtr<ID3D12Device2> owner;
  check(root->GetDevice(IID_PPV_ARGS(&owner)), "native owner");
  for (UINT i = 0; i < 2; ++i) {
    Stream s;
    s.root.payload = root.Get();
    s.depth.payload.DepthEnable = TRUE;
    s.depth.payload.DepthWriteMask = D3D12_DEPTH_WRITE_MASK_ALL;
    s.depth.payload.DepthFunc = D3D12_COMPARISON_FUNC_ALWAYS;
    s.raster.payload.FillMode = D3D12_FILL_MODE_SOLID;
    s.raster.payload.CullMode = D3D12_CULL_MODE_NONE;
    s.raster.payload.DepthClipEnable = TRUE;
    auto vs = read(i ? "mat31788.dxbc" : "mat6113.dxbc"),
         ps = read(i ? "mat31750.dxbc" : "mat523.dxbc");
    s.vs.payload = {vs.data(), vs.size()};
    s.ps.payload = {ps.data(), ps.size()};
    // Match the cooked vertex input signature, including its integer
    // ATTRIBUTE13.
    std::vector<D3D12_INPUT_ELEMENT_DESC> fields;
    for (UINT k = 0; k < (i ? 6u : 1u); ++k)
      fields.push_back({"ATTRIBUTE", k, DXGI_FORMAT_R32G32B32A32_FLOAT, 0,
                        16 * k, D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA, 0});
    fields.push_back({"ATTRIBUTE", 13, DXGI_FORMAT_R32_UINT, 0,
                      D3D12_APPEND_ALIGNED_ELEMENT,
                      D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA, 0});
    s.layout.payload = {fields.data(), static_cast<UINT>(fields.size())};
    using CreatePSO = HRESULT(WINAPI *)(
        ID3D12Device *, const D3D12_PIPELINE_STATE_STREAM_DESC *, REFIID,
        void **);
    auto make = frame_abi::method<CreatePSO>(provider.Get(), 86);
    D3D12_PIPELINE_STATE_STREAM_DESC xbox{sizeof(s), &s};
    ComPtr<ID3D12PipelineState> native;
    check(make(provider.Get(), &xbox, IID_PPV_ARGS(&native)),
          "slot 86 cooked PSO");
    std::fprintf(file, "pair_%u_slot_86=passed\n", i);
    std::fflush(file);
    auto producer = read(i ? "control1.cso" : "control0.cso");
    auto pixel = read(i ? "43ba45d6b7d30fbd.cso" : "b75039a494495b07.cso");
    s.vs.payload = {producer.data(), producer.size()};
    s.ps.payload = {pixel.data(), pixel.size()};
    s.layout.payload = {};
    D3D12_PIPELINE_STATE_STREAM_DESC pc{offsetof(Stream, extension), &s};
    ComPtr<ID3D12PipelineState> control;
    check(owner->CreatePipelineState(&pc, IID_PPV_ARGS(&control)),
          "controlled triangle PSO");
    UINT written = draw(owner.Get(), control.Get(), root.Get());
    std::fprintf(file, "pair_%u_depth_written=%u\n", i, written);
    if (written != (i ? 64u : 0u))
      throw std::runtime_error("independent pixel depth mismatch");
  }
}
int main() {
  FILE *f = nullptr;
  if (fopen_s(&f, "result.txt", "wb"))
    return 2;
  int status = 0;
  try {
    run(f);
  } catch (const std::exception &e) {
    std::fprintf(f, "error=%s\n", e.what());
    status = 1;
  }
  return fclose(f) ? 2 : status;
}
