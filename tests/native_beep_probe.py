"""Test-only MessageBeep counter inside an owned child process.

Only the child's in-memory msftedit import slot is replaced, never DLL files or
system settings. The test must terminate that child after closing the counter.
"""
import ctypes as C
from ctypes import wintypes as W
from pathlib import Path
import struct

def install(pid):
    k = C.WinDLL('kernel32', use_last_error=True)
    def bind(name, restype, args):
        f = getattr(k, name); f.restype = restype; f.argtypes = args; return f
    op = bind('OpenProcess', W.HANDLE, [W.DWORD, W.BOOL, W.DWORD])
    close = bind('CloseHandle', W.BOOL, [W.HANDLE])
    enum = bind('K32EnumProcessModules', W.BOOL, [W.HANDLE, C.c_void_p, W.DWORD, C.POINTER(W.DWORD)])
    name = bind('K32GetModuleFileNameExW', W.DWORD, [W.HANDLE, W.HMODULE, W.LPWSTR, W.DWORD])
    read = bind('ReadProcessMemory', W.BOOL, [W.HANDLE, C.c_void_p, C.c_void_p, C.c_size_t, C.c_void_p])
    write = bind('WriteProcessMemory', W.BOOL, [W.HANDLE, C.c_void_p, C.c_void_p, C.c_size_t, C.c_void_p])
    alloc = bind('VirtualAllocEx', C.c_void_p, [W.HANDLE, C.c_void_p, C.c_size_t, W.DWORD, W.DWORD])
    protect = bind('VirtualProtectEx', W.BOOL, [W.HANDLE, C.c_void_p, C.c_size_t, W.DWORD, C.POINTER(W.DWORD)])
    flush = bind('FlushInstructionCache', W.BOOL, [W.HANDLE, C.c_void_p, C.c_size_t])
    def check(ok):
        if not ok: raise C.WinError(C.get_last_error())
        return ok
    process = check(op(0x438, False, pid))
    modules = (W.HMODULE * 256)(); needed = W.DWORD()
    check(enum(process, modules, C.sizeof(modules), C.byref(needed)))
    for module in modules[:needed.value // C.sizeof(W.HMODULE)]:
        path = C.create_unicode_buffer(1024)
        check(name(process, module, path, len(path)))
        if Path(path.value).name.lower() == 'msftedit.dll':
            base = module; data = Path(path.value).read_bytes(); break
    else: raise AssertionError('No RichEdit DLL in owned test process')
    u16 = lambda at: struct.unpack_from('<H', data, at)[0]
    u32 = lambda at: struct.unpack_from('<I', data, at)[0]
    pe = u32(0x3c); opt = pe + 24
    assert u16(opt) == 0x20b
    sections = opt + u16(pe + 20)
    def offset(rva):
        for i in range(u16(pe + 6)):
            at = sections + 40*i
            va, size, raw = u32(at+12), u32(at+16), u32(at+20)
            if va <= rva < va+size: return raw+rva-va
        raise AssertionError(hex(rva))
    def string(rva):
        at = offset(rva); return data[at:data.index(b'\0', at)]
    imports = offset(u32(opt + 112 + 8))
    slot = None
    for at in range(imports, len(data), 20):
        original, _, _, dll, iat = struct.unpack_from('<5I', data, at)
        if not dll: break
        for i in range(10000):
            thunk = struct.unpack_from('<Q', data, offset(original) + i*8)[0]
            if not thunk: break
            if not thunk >> 63 and string(thunk+2) == b'MessageBeep':
                slot = base+iat+i*8; break
        if slot: break
    if not slot:
        delay = offset(u32(opt + 112 + 13*8))
        for at in range(delay, len(data), 32):
            attrs, dll, handle, iat, names, bound, unload, stamp = struct.unpack_from('<8I', data, at)
            if not dll: break
            assert attrs & 1
            for i in range(10000):
                thunk = struct.unpack_from('<Q', data, offset(names) + i*8)[0]
                if not thunk: break
                if not thunk >> 63 and string(thunk+2) == b'MessageBeep':
                    slot = base+iat+i*8; break
            if slot: break
    assert slot, 'MessageBeep import not found'
    memory = check(alloc(process, None, 4096, 0x3000, 0x40))
    code = b'\x48\xb8' + struct.pack('<Q', memory+64) + b'\xff\x00\x31\xc0\xc3'
    check(write(process, memory, code, len(code), None))
    old = W.DWORD()
    check(protect(process, slot, 8, 0x40, C.byref(old)))
    pointer = struct.pack('<Q', memory)
    check(write(process, slot, pointer, 8, None))
    previous = W.DWORD()
    check(protect(process, slot, 8, old.value, C.byref(previous)))
    check(flush(process, memory, len(code)))
    def count():
        value = W.DWORD()
        check(read(process, memory+64, C.byref(value), 4, None))
        return value.value
    return count, lambda: close(process)
