#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdarg>
#include <cstdio>
using Handler = void (WINAPI*)(const char*, const char*, unsigned, const char*, va_list);
using Register = unsigned (WINAPI*)(Handler, unsigned char);
using Remove = unsigned char (WINAPI*)(unsigned);
using RemovePtr = unsigned char (WINAPI*)(Handler);
using OnAssert = unsigned char (WINAPI*)(const char*, const char*, unsigned, const char*, va_list);
static FILE* out;
static Remove remove_handler;
static unsigned remove_a, remove_b;
static bool modify;
static void print(char id, const char* expr, const char* file, unsigned line, const char* format, va_list args) {
    char message[100]; vsprintf_s(message, format, args);
    fprintf(out, "%c:%s,%s,%u,%s;", id, expr, file, line, message);
}
static void WINAPI a(const char* x, const char* y, unsigned z, const char* f, va_list v) { print('A', x, y, z, f, v); }
static void WINAPI b(const char* x, const char* y, unsigned z, const char* f, va_list v) { print('B', x, y, z, f, v); }
static void WINAPI c(const char* x, const char* y, unsigned z, const char* f, va_list v) {
    print('C', x, y, z, f, v);
    if (modify) { remove_handler(remove_a); remove_handler(remove_b); }
}
static unsigned char invoke(OnAssert handler, const char* format, ...) {
    va_list args; va_start(args, format);
    auto result = handler("condition", "file.cpp", 42, format, args);
    va_end(args); return result;
}
int main(int argc, char** argv) {
    if (argc != 2 || fopen_s(&out, argv[1], "wb")) return 2;
    HMODULE dll = LoadLibraryW(L"xtfbaseservices.dll"); if (!dll) return 3;
    auto reg = reinterpret_cast<Register>(GetProcAddress(dll, "XtfErrRegisterAssertHandlerPtr"));
    remove_handler = reinterpret_cast<Remove>(GetProcAddress(dll, "XtfErrRemoveAssertHandler"));
    auto remptr = reinterpret_cast<RemovePtr>(GetProcAddress(dll, "XtfErrRemoveAssertHandlerByPtr"));
    auto on = reinterpret_cast<OnAssert>(GetProcAddress(dll, "XtfErrOnAssertV"));
    if (!reg || !remove_handler || !remptr || !on) return 4;
    auto null_id = reg(nullptr, 0);
    auto id_a = reg(a, 2); auto id_b = reg(b, 0); auto id_c = reg(c, 1);
    auto repeat_b = reg(b, 0); auto changed_b = reg(b, 2);
    fprintf(out, "ids=%u,%u,%u,%u,%u,%u\n", null_id, id_a, id_b, id_c, repeat_b, changed_b);
    fprintf(out, "dispatch="); auto result = invoke(on, "value=%d", 37); fprintf(out, "ret:%u\n", result);
    remove_a = id_a; remove_b = changed_b; modify = true;
    fprintf(out, "snapshot="); result = invoke(on, "value=%d", 38); fprintf(out, "ret:%u\n", result);
    modify = false;
    fprintf(out, "after="); result = invoke(on, "value=%d", 39); fprintf(out, "ret:%u\n", result);
    auto by_id = remove_handler(id_c); auto missing_id = remove_handler(id_c);
    auto by_ptr = remptr(c); auto missing_ptr = remptr(nullptr);
    fprintf(out, "remove=%u,%u,%u,%u\n", by_id, missing_id, by_ptr, missing_ptr);
    return fclose(out);
}
