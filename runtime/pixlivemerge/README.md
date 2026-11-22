# PIX live merge

`pixlivemerge.dll` currently provides `FindPdbInfo` (ordinal 2), one of its six exports. It searches a vector of PDB records and returns the first record whose name is a suffix of the requested path, or -1. The comparison ignores ASCII letter case; other UTF-16 units compare exactly. An empty name matches any non-null path.

This interface uses the x64 MSVC `std::vector` and `std::wstring` representations: records are 88 bytes and begin with a string. The caller must supply valid terminated UTF-16 strings and a valid vector. The local control `tools/check_pixlivemerge.py` compares the provider with a supplied original DLL, including short and allocated strings, duplicates, null and empty inputs, and case behavior. Its receipt is in `evidence/providers/pixlivemerge-find-pdb.json`.

This does not parse PDB files or implement a PIX capture backend. The remaining live-merge exports are not implemented; this provider is not staged for games by default.
