#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdio>
#include <string>
#include <vector>

struct PdbInfo { std::wstring name; GUID guid; unsigned age; std::string path; };
static_assert(sizeof(PdbInfo) == 88, "requires x64 MSVC ABI");
using Parse = HRESULT (WINAPI*)(wchar_t*, size_t, std::vector<PdbInfo>*);

static void mutation_case(FILE* out, Parse parse, const char* label, const wchar_t* text) {
    std::wstring source(text);
    std::vector<wchar_t> contents(source.c_str(), source.c_str() + source.size() + 1);
    std::vector<PdbInfo> entries;
    HRESULT result = parse(contents.data(), contents.size(), &entries);
    fprintf(out, "%s=%08lx,%zu,", label, result, entries.size());
    for (size_t i = 0; i < source.size(); ++i) if (contents[i] != source[i])
        fprintf(out, "%zu:%04x>%04x;", i, static_cast<unsigned>(source[i]), static_cast<unsigned>(contents[i]));
    fprintf(out, "\n");
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    FILE* out = nullptr;
    if (fopen_s(&out, argv[1], "wb")) return 3;
    HMODULE dll = LoadLibraryW(L"pixlivemerge.dll");
    if (!dll) return 4;
    auto parse = reinterpret_cast<Parse>(GetProcAddress(dll, "ParsePdbInfo"));
    if (!parse) return 5;
    mutation_case(out, parse, "bad_guid", L"foo@{invalid} 7 file.pdb");
    mutation_case(out, parse, "missing_close", L"foo@{12345678-1234-1234-1234-123456789abc 7 file.pdb");
    mutation_case(out, parse, "no_path", L"foo@{12345678-1234-1234-1234-123456789abc} 7");
    std::wstring sample = L"one@{12345678-1234-1234-1234-123456789abc} 42 first.pdb";
    auto input = std::vector<wchar_t>(sample.c_str(), sample.c_str() + sample.size() + 1);
    std::vector<PdbInfo> initial;
    HRESULT hr = parse(input.data(), input.size(), &initial);
    fprintf(out, "parse=%08lx,%zu\nmutation=", hr, initial.size());
    for (size_t i = 0; i < sample.size(); ++i) if (input[i] != sample[i]) {
        fprintf(out, "%zu:%04x>%04x,", i, static_cast<unsigned>(sample[i]), static_cast<unsigned>(input[i]));
    }
    fprintf(out, "\npath_length=%zu\n", initial.empty() ? 0 : initial[0].path.size());
    bool good = hr == S_OK && initial.size() == 1;
    {
        std::vector<PdbInfo> spare;
        spare.reserve(4);
        spare.push_back({L"existing", {}, 7, "existing.pdb"});
        auto buf = std::vector<wchar_t>(sample.c_str(), sample.c_str() + sample.size() + 1);
        auto result = parse(buf.data(), buf.size(), &spare);
        fprintf(out, "spare=%08lx,%zu,%ls,%ls\n", result, spare.size(), spare[0].name.c_str(), spare[1].name.c_str());
    }
    for (unsigned i = 0; good && i < 100; ++i) {
        std::wstring name(3000, L'n');
        std::wstring source = name + L"@{12345678-1234-1234-1234-123456789abc} 42 long-path.pdb";
        std::vector<wchar_t> contents(source.c_str(), source.c_str() + source.size() + 1);
        std::vector<PdbInfo> entries;
        entries.reserve(1);
        entries.push_back({L"existing", {}, 7, "existing.pdb"});
        HRESULT test = parse(contents.data(), contents.size(), &entries);
        good = test == S_OK && entries.size() == 2;
        if (!good) fprintf(out, "failure_parse=%08lx,%zu\n", test, entries.size());
        if (!good) break;
        good = entries[1].name == L"existing" && entries[0].name == name &&
               entries[0].age == 42 && entries[0].path.size() == 780 && entries[0].path.compare(0, 13, "long-path.pdb") == 0;
        if (i == 0 && good) fprintf(out, "growth=parsed,existing\n");
        if (!good) fprintf(out, "failure_content=%zu,%u,%s;%zu,%u,%s;eq=%d,%d,%d,%d\n", entries[0].name.size(), entries[0].age, entries[0].path.c_str(), entries[1].name.size(), entries[1].age, entries[1].path.c_str(), entries[1].name == L"existing", entries[0].name == name, entries[0].age == 42, entries[0].path == "long-path.pdb");
        entries[0].name.append(4096, L'x');
        entries[0].path.append(4096, 'y');
        entries.reserve(50);
        entries.clear();
        BOOL heap_ok = HeapValidate(GetProcessHeap(), 0, nullptr);
        if (!heap_ok) fprintf(out, "failure_heap=%lu\n", GetLastError());
        good = good && heap_ok;
    }
    initial.clear();
    fprintf(out, "ownership_stress=%d\n", good ? 1 : 0);
    return fclose(out) || !good;
}
