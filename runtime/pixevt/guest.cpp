#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <array>
#include <cstdio>

int wmain(int argc, wchar_t** argv) {
    if (argc != 2) return 2;
    HMODULE module = LoadLibraryW(L"pixevt.dll");
    if (!module) return 3;
    struct Export { const char* name; WORD ordinal; };
    constexpr Export exports[] = {{"PIXBeginCapture1",5},{"PIXBeginCapture2",6},{"PIXEndCapture",7},
                                  {"PIXGetCaptureState",9},{"PIXReportCounter",10}};
    for (const auto& e : exports) {
        auto name = GetProcAddress(module,e.name);
        if (!name || name != GetProcAddress(module,MAKEINTRESOURCEA(e.ordinal))) return 4;
    }
    using Begin = HRESULT (WINAPI*)(DWORD,const void*);
    using End = HRESULT (WINAPI*)(BOOL);
    using State = DWORD (WINAPI*)();
    using Counter = void (WINAPI*)(const wchar_t*,float);
    auto begin1 = reinterpret_cast<Begin>(GetProcAddress(module,"PIXBeginCapture1"));
    auto begin2 = reinterpret_cast<Begin>(GetProcAddress(module,"PIXBeginCapture2"));
    auto end = reinterpret_cast<End>(GetProcAddress(module,"PIXEndCapture"));
    auto state = reinterpret_cast<State>(GetProcAddress(module,"PIXGetCaptureState"));
    auto counter = reinterpret_cast<Counter>(GetProcAddress(module,"PIXReportCounter"));
    std::array<uint32_t,4> guard{0xdeadbeef,0x12345678,0xabcdef01,0x6d5c4b3a};
    const auto expected = guard;
    for (DWORD flag : {0u,1u,2u,0xffffffffu}) {
        if (state() != 0 || begin1(flag,nullptr) != E_NOTIMPL || begin1(flag,guard.data()) != E_NOTIMPL ||
            begin2(flag,nullptr) != E_UNEXPECTED || begin2(flag,guard.data()) != E_UNEXPECTED ||
            end(FALSE) != E_UNEXPECTED || end(TRUE) != E_UNEXPECTED || state() != 0 || guard != expected) return 5;
    }
    counter(L"frame-cost-ms",2.25f);
    counter(L"queue-depth",17.0f);
    FILE* f = nullptr;
    if (_wfopen_s(&f,argv[1],L"wb")) return 6;
    int wrote = std::fprintf(f,"no_capture=1\nlegacy=E_NOTIMPL\nbegin2=E_UNEXPECTED\nend=E_UNEXPECTED\nflags=4\nguards=4\ncounters=2\n");
    return std::fclose(f) || wrote < 0;
}
