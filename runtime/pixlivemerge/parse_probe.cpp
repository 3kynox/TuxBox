#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdio>
#include <string>
#include <vector>

struct PdbInfo { std::wstring name; GUID guid; unsigned age; std::string path; };
static_assert(sizeof(PdbInfo) == 88, "requires x64 MSVC ABI");
using Parse = HRESULT (WINAPI*)(wchar_t*, size_t, std::vector<PdbInfo>*);

static void record(FILE* out, Parse parse, const char* label, std::vector<wchar_t> data) {
    std::vector<PdbInfo> entries;
    const HRESULT result = parse(data.data(), data.size(), &entries);
    fprintf(out, "%s=%08lx,%zu", label, result, entries.size());
    for (const auto& entry : entries) {
        fprintf(out, ",%ls:%08lx:%u:%s", entry.name.c_str(), entry.guid.Data1,
                entry.age, entry.path.c_str());
    }
    fprintf(out, "\n");
}

static std::vector<wchar_t> buffer(const wchar_t* value) {
    std::wstring s(value);
    return std::vector<wchar_t>(s.c_str(), s.c_str() + s.size() + 1);
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    FILE* out = nullptr;
    if (fopen_s(&out, argv[1], "wb")) return 3;
    HMODULE dll = LoadLibraryW(L"pixlivemerge.dll");
    if (!dll) return 4;
    auto parse = reinterpret_cast<Parse>(GetProcAddress(dll, "ParsePdbInfo"));
    if (!parse) return 5;
    record(out, parse, "empty", buffer(L""));
    record(out, parse, "malformed", buffer(L"junk"));
    record(out, parse, "valid", buffer(L"a@{12345678-1234-1234-1234-123456789abc} 42 test.pdb"));
    record(out, parse, "missing_space", buffer(L"a@{12345678-1234-1234-1234-123456789abc}42 test"));
    wchar_t multi[] = L"one@{00000000-0000-0000-0000-000000000000} 1 first.pdb\0two@{12345678-1234-1234-1234-123456789abc} 2 second.pdb\0";
    record(out, parse, "multi", std::vector<wchar_t>(multi, multi + sizeof(multi)/sizeof(*multi)));
    return fclose(out);
}
