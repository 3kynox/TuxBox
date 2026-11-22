#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdio>
#include <string>
#include <vector>

struct Entry { std::wstring name; unsigned char other[56]{}; };
static_assert(sizeof(Entry) == 88, "requires the MSVC x64 string ABI");
using Find = int (WINAPI*)(const wchar_t*, const std::vector<Entry>*);
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    FILE* f = nullptr;
    if (fopen_s(&f, argv[1], "wb")) return 3;
    HMODULE lib = LoadLibraryW(L"pixlivemerge.dll");
    if (!lib) return 4;
    auto symbol = GetProcAddress(lib, "FindPdbInfo");
    if (!symbol || symbol != GetProcAddress(lib, MAKEINTRESOURCEA(2))) return 5;
    auto find = reinterpret_cast<Find>(symbol);
    std::vector<Entry> v;
    v.push_back({L"foo.pdb", {}});
    v.push_back({L"really-long-test.pdb", {}});
    v.push_back({L"FOO.PDB", {}});
    v.push_back({L"\u00c4.pdb", {}});
    for (auto& entry : v) for (auto& b : entry.other) b = 0xa5;
    fprintf(f, "short=%d\nlong=%d\ncase=%d\nfirst=%d\nmiss=%d\nnull=%d\nnonascii_equal=%d\nnonascii_case=%d\n",
        find(L"C:\\dir\\FOO.PDB", &v), find(L"C:\\really-long-test.pdb", &v),
        find(L"foO.Pdb", &v), find(L"prefixfoo.pdb", &v), find(L"other", &v),
        find(nullptr, &v), find(L"\u00c4.PDB", &v), find(L"\u00e4.pdb", &v));
    std::vector<Entry> empty; empty.reserve(3);
    std::vector<Entry> blank; blank.push_back({L"", {}});
    fprintf(f, "empty=%d\nblank=%d\nblank_path=%d\n", find(L"anything", &empty),
        find(L"anything", &blank), find(L"", &blank));
    bool intact = true;
    for (auto& entry : v) for (auto b : entry.other) intact &= b == 0xa5;
    fprintf(f, "records_intact=%d\n", intact ? 1 : 0);
    return fclose(f);
}
