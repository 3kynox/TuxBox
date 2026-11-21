// Desktop-host control for two GameConfigHelper exports. Compile once and run
// the same executable beside either DLL in separate directories.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <winstring.h>
#include <stdio.h>
#include <wchar.h>

__declspec(dllimport) HRESULT WINAPI GetRegistryPathForPackage(PCWSTR, PCWSTR, HSTRING *);
__declspec(dllimport) HRESULT WINAPI OpenGameConfigKeyForPackage(PCWSTR, REGSAM, PCWSTR, HKEY *);

static const wchar_t base[] = L"Software\\Microsoft\\GamingServices\\GameConfig\\";
static const wchar_t package[] = L"TuxBox.GameConfig_1.0.0.0_x64__test_\u2603";

static BOOL local_provider(void) {
    wchar_t exe[MAX_PATH], dll[MAX_PATH];
    HMODULE module = GetModuleHandleW(L"gameconfighelper.dll");
    DWORD exe_len = GetModuleFileNameW(NULL, exe, ARRAYSIZE(exe));
    DWORD dll_len = module ? GetModuleFileNameW(module, dll, ARRAYSIZE(dll)) : 0;
    if (!exe_len || exe_len >= ARRAYSIZE(exe) || !dll_len || dll_len >= ARRAYSIZE(dll)) return FALSE;
    wchar_t *exe_name = wcsrchr(exe, L'\\'), *dll_name = wcsrchr(dll, L'\\');
    if (!exe_name || !dll_name || _wcsicmp(dll_name + 1, L"gameconfighelper.dll")) return FALSE;
    *exe_name = *dll_name = L'\0';
    return !_wcsicmp(exe, dll)
        && GetProcAddress(module, "GetRegistryPathForPackage") == GetProcAddress(module, MAKEINTRESOURCEA(13))
        && GetProcAddress(module, "OpenGameConfigKeyForPackage") == GetProcAddress(module, MAKEINTRESOURCEA(17));
}

static BOOL strings(void) {
    // On a PC the optional user path does not change the registry subkey.
    wchar_t expected[512];
    if (swprintf_s(expected, ARRAYSIZE(expected), L"%ls%ls", base, package) < 0) return FALSE;
    HSTRING first = NULL, second = NULL, empty = NULL, null_name = NULL;
    wchar_t mutable_name[ARRAYSIZE(package)];
    wcscpy_s(mutable_name, ARRAYSIZE(mutable_name), package);
    HRESULT a = GetRegistryPathForPackage(mutable_name, NULL, &first);
    mutable_name[0] = L'X';
    HRESULT b = GetRegistryPathForPackage(package, L"UnusedOnDesktop", &second);
    HRESULT c = GetRegistryPathForPackage(L"", NULL, &empty);
    HRESULT d = GetRegistryPathForPackage(NULL, NULL, &null_name);
    UINT32 length = 0;
    PCWSTR value = WindowsGetStringRawBuffer(first, &length);
    BOOL ok = a == S_OK && b == S_OK && c == S_OK && d == S_OK && length == wcslen(expected)
        && wcscmp(value, expected) == 0
        && wcscmp(WindowsGetStringRawBuffer(second, NULL), expected) == 0
        && wcscmp(WindowsGetStringRawBuffer(empty, NULL), base) == 0
        && wcscmp(WindowsGetStringRawBuffer(null_name, NULL),
                  L"Software\\Microsoft\\GamingServices\\GameConfig\\(null)") == 0
        && GetRegistryPathForPackage(package, NULL, NULL) == E_INVALIDARG;
    WindowsDeleteString(null_name);
    WindowsDeleteString(empty);
    WindowsDeleteString(second);
    WindowsDeleteString(first);
    if (!ok) return FALSE;

    // The complete path fits exactly 511 characters plus its terminator.
    size_t limit = 511 - wcslen(base);
    wchar_t name[512];
    wmemset(name, L'x', limit + 1);
    name[limit] = 0;
    HSTRING max = NULL;
    if (GetRegistryPathForPackage(name, NULL, &max) != S_OK) return FALSE;
    value = WindowsGetStringRawBuffer(max, &length);
    ok = length == 511 && wcsncmp(value, base, wcslen(base)) == 0
        && wcscmp(value + wcslen(base), name) == 0;
    name[limit] = L'x';
    name[limit + 1] = 0;
    HSTRING unchanged = max;
    HRESULT too_long = GetRegistryPathForPackage(name, NULL, &unchanged);
    ok = ok && too_long == HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER) && unchanged == max;
    WindowsDeleteString(max);
    return ok;
}

static BOOL populate(HKEY hive, DWORD marker) {
    wchar_t path[512];
    if (swprintf_s(path, ARRAYSIZE(path), L"%ls%ls", base, package) < 0) return FALSE;
    HKEY key = NULL;
    if (RegCreateKeyExW(hive, path, 0, NULL, REG_OPTION_VOLATILE, KEY_SET_VALUE, NULL, &key, NULL)) return FALSE;
    LSTATUS status = RegSetValueExW(key, L"Marker", 0, REG_DWORD, (const BYTE *)&marker, sizeof(marker));
    RegCloseKey(key);
    return status == ERROR_SUCCESS;
}

static BOOL open_marker(PCWSTR user, DWORD expected) {
    HKEY key = NULL;
    if (OpenGameConfigKeyForPackage(package, KEY_QUERY_VALUE, user, &key) != S_OK || !key)
        return FALSE;
    DWORD marker = 0, type = 0, size = sizeof(marker);
    LSTATUS status = RegQueryValueExW(key, L"Marker", NULL, &type, (BYTE *)&marker, &size);
    RegCloseKey(key);
    return status == ERROR_SUCCESS && type == REG_DWORD && size == sizeof(marker)
        && marker == expected;
}

static BOOL registry(void) {
    // Override both predefined roots in this process so the DLL can only open
    // synthetic keys. Every write is scoped to a temporary current-user key.
    wchar_t path[128];
    if (swprintf_s(path, ARRAYSIZE(path), L"Software\\TuxBox-GameConfigControl-%lu",
                   GetCurrentProcessId()) < 0) return FALSE;
    HKEY root = NULL, machine = NULL, users = NULL, missing = NULL;
    BOOL result = FALSE, machine_override = FALSE, users_override = FALSE;
    if (RegCreateKeyExW(HKEY_CURRENT_USER, path, 0, NULL, REG_OPTION_VOLATILE,
                        KEY_ALL_ACCESS, NULL, &root, NULL)) return FALSE;
    if (RegCreateKeyExW(root, L"Machine", 0, NULL, REG_OPTION_VOLATILE, KEY_ALL_ACCESS, NULL, &machine, NULL)
        || RegCreateKeyExW(root, L"Users", 0, NULL, REG_OPTION_VOLATILE, KEY_ALL_ACCESS, NULL, &users, NULL)
        || !populate(machine, 0x1234) || !populate(users, 0x5678)) goto cleanup;
    if (RegOverridePredefKey(HKEY_LOCAL_MACHINE, machine)) goto cleanup;
    machine_override = TRUE;
    if (RegOverridePredefKey(HKEY_USERS, users)) goto cleanup;
    users_override = TRUE;
    result = open_marker(NULL, 0x1234) && open_marker(L"UnusedOnDesktop", 0x5678)
        && OpenGameConfigKeyForPackage(L"NoSuchPackage", KEY_QUERY_VALUE, NULL, &missing)
            == HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
cleanup:
    if (missing) RegCloseKey(missing);
    if (users_override) RegOverridePredefKey(HKEY_USERS, NULL);
    if (machine_override) RegOverridePredefKey(HKEY_LOCAL_MACHINE, NULL);
    if (users) RegCloseKey(users);
    if (machine) RegCloseKey(machine);
    RegCloseKey(root);
    RegDeleteTreeW(HKEY_CURRENT_USER, path);
    return result;
}

int wmain(int argc, wchar_t **argv) {
    if (argc != 1 && (argc != 3 || wcscmp(argv[1], L"--report") || !*argv[2])) return 2;
    if (!local_provider()) { fputs("Expected the app-local GameConfigHelper DLL and its ordinals\n", stderr); return 1; }
    if (!strings()) { fputs("GameConfig registry string comparison failed\n", stderr); return 1; }
    if (!registry()) { fputs("GameConfig registry key comparison failed\n", stderr); return 1; }
    const char *receipt = "gameconfig-query PASS paths=7 hives=2 missing-key=1";
    if (argc == 3) {
        FILE *report;
        if (_wfopen_s(&report, argv[2], L"wb")) return 1;
        int written = fprintf(report, "%s\n", receipt);
        if (fclose(report) || written < 0) return 1;
    }
    puts(receipt);
    return 0;
}
