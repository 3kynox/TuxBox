#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdarg>
#include <cstdio>
#include <cstring>
using Handler = void (WINAPI*)(const char*, const char*, unsigned, const char*, va_list);
using Register = unsigned (WINAPI*)(Handler, unsigned char);
using OnAssert = unsigned char (WINAPI*)(const char*, const char*, unsigned, const char*, va_list);
static OnAssert on_assert;
static volatile LONG breaks;
static FILE* debug_out;
static LONG debug_count;
static void WINAPI debug_capture(const char* message) {
    fprintf(debug_out, "debug%ld=%s\n", InterlockedIncrement(&debug_count), message ? message : "<null>"); fflush(debug_out);
}
static bool intercept_debug(HMODULE module) {
    auto base = reinterpret_cast<unsigned char*>(module);
    auto dos = reinterpret_cast<IMAGE_DOS_HEADER*>(base);
    auto nt = reinterpret_cast<IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
    auto imports = reinterpret_cast<IMAGE_IMPORT_DESCRIPTOR*>(base + nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress);
    for (auto desc = imports; desc->Name; ++desc) {
        auto original = reinterpret_cast<IMAGE_THUNK_DATA*>(base + desc->OriginalFirstThunk);
        auto table = reinterpret_cast<IMAGE_THUNK_DATA*>(base + desc->FirstThunk);
        for (; original->u1.AddressOfData; ++original, ++table) {
            if (IMAGE_SNAP_BY_ORDINAL(original->u1.Ordinal)) continue;
            auto name = reinterpret_cast<IMAGE_IMPORT_BY_NAME*>(base + original->u1.AddressOfData);
            if (strcmp(reinterpret_cast<const char*>(name->Name), "OutputDebugStringA")) continue;
            DWORD old; if (!VirtualProtect(&table->u1.Function, sizeof(table->u1.Function), PAGE_READWRITE, &old)) return false;
            table->u1.Function = reinterpret_cast<ULONGLONG>(&debug_capture);
            DWORD ignored; VirtualProtect(&table->u1.Function, sizeof(table->u1.Function), old, &ignored);
            return true;
        }
    }
    return false;
}
static HANDLE entered, proceed;
static unsigned char nested_result = 255;
static bool recursive;
static LONG CALLBACK exception_handler(EXCEPTION_POINTERS* exception) {
    if (exception->ExceptionRecord->ExceptionCode != EXCEPTION_BREAKPOINT) return EXCEPTION_CONTINUE_SEARCH;
    auto count = InterlockedIncrement(&breaks);
    if (count > 2) ExitProcess(20);
    ++exception->ContextRecord->Rip;
    return EXCEPTION_CONTINUE_EXECUTION;
}
static unsigned char invoke(const char* format, ...) {
    va_list args; va_start(args, format);
    auto result = on_assert("condition", "file.cpp", 42, format, args);
    va_end(args); return result;
}
static void WINAPI callback(const char*, const char*, unsigned, const char*, va_list) {
    if (recursive) { recursive = false; nested_result = invoke("recursive"); }
    else if (entered) { SetEvent(entered); WaitForSingleObject(proceed, 10000); }
}
static DWORD WINAPI worker(void* result) {
    *static_cast<unsigned char*>(result) = invoke("worker");
    return 0;
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    FILE* out = nullptr; if (fopen_s(&out, argv[1], "wb")) return 3;
    HMODULE dll = LoadLibraryW(L"xtfbaseservices.dll"); if (!dll) return 4;
    debug_out = out; if (!intercept_debug(dll)) return 8;
    auto reg = reinterpret_cast<Register>(GetProcAddress(dll, "XtfErrRegisterAssertHandlerPtr"));
    on_assert = reinterpret_cast<OnAssert>(GetProcAddress(dll, "XtfErrOnAssertV"));
    if (!reg || !on_assert || !AddVectoredExceptionHandler(1, exception_handler)) return 5;
    auto id = reg(callback, 0);
    auto set_level = reinterpret_cast<long (WINAPI*)(unsigned)>(GetProcAddress(dll, "XtfLogSetMainLevel"));
    if (!set_level) return 9;
    auto set_result = set_level(5);
    auto normal = invoke("plain assertion message");
    fprintf(out, "normal=%08lx,%u,%ld\n", set_result, normal, debug_count);
    fprintf(out, "start=1\n"); fflush(out);
    recursive = true;
    unsigned char outer = invoke("outer");
    fprintf(out, "recursive=%u,%u,%ld\n", outer, nested_result, breaks); fflush(out);
    entered = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    proceed = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    unsigned char worker_result = 255;
    HANDLE thread = CreateThread(nullptr, 0, worker, &worker_result, 0, nullptr);
    if (!entered || !proceed || !thread || WaitForSingleObject(entered, 5000) != WAIT_OBJECT_0) return 6;
    unsigned char concurrent = invoke("concurrent");
    SetEvent(proceed);
    if (WaitForSingleObject(thread, 5000) != WAIT_OBJECT_0) return 7;
    fprintf(out, "concurrent=%u,%u,%ld\nid=%u\n", worker_result, concurrent, breaks, id);
    CloseHandle(thread); CloseHandle(entered); CloseHandle(proceed);
    return fclose(out);
}
