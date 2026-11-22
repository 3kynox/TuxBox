#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdio>
using Get = unsigned char (WINAPI*)();
using Push = void (WINAPI*)(unsigned char);
using Pop = void (WINAPI*)();
static Get get_state;
static Push push_state;
static Pop pop_state;
static DWORD WINAPI check_thread(void* p) {
    auto out = static_cast<int*>(p);
    out[0] = get_state(); push_state(1); out[1] = get_state(); pop_state(); out[2] = get_state();
    return 0;
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    FILE* out = nullptr;
    if (fopen_s(&out, argv[1], "wb")) return 3;
    HMODULE dll = LoadLibraryW(L"xtfbaseservices.dll");
    if (!dll) return 4;
    FARPROC get = GetProcAddress(dll, "XtfErrAreAssertsEnabled");
    FARPROC push = GetProcAddress(dll, "XtfErrPushAssertsEnabled");
    FARPROC pop = GetProcAddress(dll, "XtfErrPopAssertsEnabled");
    if (!get || !push || !pop || get != GetProcAddress(dll, MAKEINTRESOURCEA(44)) ||
        pop != GetProcAddress(dll, MAKEINTRESOURCEA(46)) || push != GetProcAddress(dll, MAKEINTRESOURCEA(47))) return 5;
    get_state = reinterpret_cast<Get>(get); push_state = reinterpret_cast<Push>(push); pop_state = reinterpret_cast<Pop>(pop);
    fprintf(out, "initial=%u\n", get_state());
    pop_state(); fprintf(out, "underflow=%u\n", get_state());
    push_state(0); fprintf(out, "disable=%u\n", get_state());
    push_state(1); fprintf(out, "nested=%u\n", get_state());
    pop_state(); fprintf(out, "restore1=%u\n", get_state());
    int values[3] = {};
    HANDLE thread = CreateThread(nullptr, 0, check_thread, values, 0, nullptr);
    if (!thread || WaitForSingleObject(thread, INFINITE) != WAIT_OBJECT_0) return 6;
    CloseHandle(thread);
    fprintf(out, "thread=%d,%d,%d\nafter_thread=%u\n", values[0], values[1], values[2], get_state());
    pop_state(); fprintf(out, "restore2=%u\n", get_state());
    for (int i = 0; i < 100; ++i) push_state(static_cast<unsigned char>(i & 1));
    bool deep = true;
    for (int i = 99; i >= 0; --i) { deep &= get_state() == (i & 1); pop_state(); }
    fprintf(out, "deep=%d,%u\n", deep ? 1 : 0, get_state());
    FARPROC type_symbol = GetProcAddress(dll, "XtfLogGetDebugType");
    FARPROC color_symbol = GetProcAddress(dll, "XtfLogGetTextColors");
    if (!type_symbol || !color_symbol || type_symbol != GetProcAddress(dll, MAKEINTRESOURCEA(54)) ||
        color_symbol != GetProcAddress(dll, MAKEINTRESOURCEA(56))) return 7;
    auto type = reinterpret_cast<const char* (WINAPI*)(unsigned)>(type_symbol);
    auto color = reinterpret_cast<unsigned short (WINAPI*)(unsigned)>(color_symbol);
    for (unsigned i = 0; i < 8; ++i) {
        const char* label = type(i);
        fprintf(out, "level%u=%s,%u\n", i, label ? label : "<null>", color(i));
    }
    fprintf(out, "large=%s,%u\n", type(~0u) ? type(~0u) : "<null>", color(~0u));
    return fclose(out);
}
